use crate::core::document::Document;
use crate::core::piece_table::CellId;
use crate::error::{HxError, HxResult};
use crate::mode::NibblePhase;

use super::walk::WalkControl;

/// A single cell's replacement change: `(cell, before, after)` where each
/// replacement value is `None` when the cell shows its base byte. Returned by
/// the streaming in-place transforms so callers can build an undo record.
pub type ReplacementDelta = (CellId, Option<u8>, Option<u8>);

const REPLACEMENT_CHUNK: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactReplacementStats {
    pub visited: u64,
    pub changed: u64,
}

impl Document {
    /// Get the current replacement value for a cell (used by undo to snapshot
    /// the "before" state).
    pub fn replacement_state(&self, id: CellId) -> Option<u8> {
        self.replacements.get(&id).copied()
    }

    /// Restore a replacement to its previous state (used by undo).
    pub fn restore_replacement(&mut self, id: CellId, previous: Option<u8>) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        match previous {
            Some(value) => {
                self.replacements.insert(id, value);
            }
            None => {
                self.replacements.remove(&id);
            }
        }
        Ok(())
    }

    /// True when a display range contains no tombstones and no replacement
    /// entries. Such ranges can use compact undo records whose "before" state
    /// is represented as clearing replacements rather than storing one delta
    /// per byte.
    pub fn replacement_range_is_pristine(&self, offset: u64, len: u64) -> bool {
        if len == 0 || offset >= self.len() {
            return true;
        }
        let end = offset.saturating_add(len).min(self.len());
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        if !has_tombstones && !has_replacements {
            return true;
        }

        let mut display_cursor = 0_u64;
        for piece in self.pieces_snapshot() {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor.saturating_add(piece.len);
            if piece_end <= offset {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = offset.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_offset = piece.start + (overlap_start - display_cursor);
                let range_len = overlap_end - overlap_start;
                let lo = CellId::from_source(piece.source, source_offset);
                let hi = CellId::from_source(piece.source, source_offset + range_len - 1);
                if (has_tombstones && self.has_tombstone_in_range(lo, hi))
                    || (has_replacements && self.has_replacement_in_range(lo, hi))
                {
                    return false;
                }
            }
            display_cursor = piece_end;
        }
        true
    }

    /// Tombstone-delete a byte (normal/visual mode). The cell keeps its
    /// display slot but renders as `Deleted` and is skipped on save.
    pub fn mark_tombstone(&mut self, offset: u64) -> HxResult<Option<CellId>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if self.fixed_size {
            return Err(HxError::FixedSizeViolation);
        }
        let id = self.cell_id_at(offset).ok_or(HxError::OffsetOutOfRange)?;
        Ok(self.tombstones.insert(id).then_some(id))
    }

    /// Remove tombstones (used by undo of tombstone-delete).
    pub fn clear_tombstones(&mut self, ids: &[CellId]) {
        for id in ids {
            self.tombstones.remove(id);
        }
    }

    /// Re-apply tombstones for a set of stable cells (used by redo).
    pub fn mark_tombstones(&mut self, ids: &[CellId]) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if self.fixed_size {
            return Err(HxError::FixedSizeViolation);
        }
        for id in ids {
            self.tombstones.insert(*id);
        }
        Ok(())
    }

    /// Replace a single nibble (high or low) of the byte at `offset`.
    /// Used by edit-mode hex input. If `offset == len`, inserts a new byte
    /// (only valid for the high nibble).
    pub fn replace_nibble(
        &mut self,
        offset: u64,
        phase: NibblePhase,
        nibble: u8,
    ) -> HxResult<CellId> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset == self.len() {
            if self.fixed_size {
                return Err(HxError::FixedSizeViolation);
            }
            if matches!(phase, NibblePhase::High) {
                return self.insert_byte(offset, nibble << 4);
            }
            return Err(HxError::OffsetOutOfRange);
        }

        let (id, current) = self.display_byte_for_edit(offset)?;
        let updated = match phase {
            NibblePhase::High => (nibble << 4) | (current & 0x0f),
            NibblePhase::Low => (current & 0xf0) | nibble,
        };
        self.set_display_byte_by_id(id, updated)?;
        Ok(id)
    }

    /// Replace the entire byte at `offset` with `value`.
    /// Used by insert-mode to fill in the low nibble of a pending byte.
    pub fn replace_display_byte(&mut self, offset: u64, value: u8) -> HxResult<CellId> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let (id, _) = self.display_byte_for_edit(offset)?;
        self.set_display_byte_by_id(id, value)?;
        Ok(id)
    }

    /// Replace the byte identified by `id` with `value`, skipping the display
    /// offset → cell resolution. Used by bulk overwrite paths that have
    /// already resolved the cell (e.g. overwrite-paste walking pieces).
    pub fn replace_display_byte_by_id(&mut self, id: CellId, value: u8) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if self.tombstones.contains(&id) {
            return Err(HxError::OffsetOutOfRange);
        }
        self.set_display_byte_by_id(id, value)
    }

    /// Apply an in-place transform to every visible byte in a display range,
    /// streaming through the piece list in 64 KB chunks so a multi-GB
    /// selection never materializes its bytes twice in memory.
    ///
    /// Tombstoned cells are skipped (they carry no logical byte), matching the
    /// `logical_bytes` view. Each visited cell's current display byte (base
    /// byte with any replacement applied) is passed to `transform`; the result
    /// is written back as a replacement. Returns one
    /// `(cell, before_replacement, after_replacement)` entry per cell whose
    /// replacement state actually changed, plus the total count of visible
    /// (non-tombstone) cells visited, ready for the caller's undo record and
    /// status reporting.
    ///
    /// Pure replacement semantics: never inserts, tombstones, or real-deletes.
    pub fn transform_visible_range_in_place(
        &mut self,
        start: u64,
        end_inclusive: u64,
        mut transform: impl FnMut(u8) -> u8,
    ) -> HxResult<(u64, Vec<ReplacementDelta>)> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok((0, Vec::new()));
        }

        let mut changes = Vec::new();
        let mut visited = 0_u64;
        self.walk_visible_cells(start, end_inclusive, 64 * 1024, |document, chunk| {
            for cell in chunk.cells {
                if cell.deleted {
                    continue;
                }
                visited += 1;
                let updated = transform(cell.byte);
                let before = document.replacement_state(cell.id);
                document.set_display_byte_by_id(cell.id, updated)?;
                let after = document.replacement_state(cell.id);
                if after != before {
                    changes.push((cell.id, before, after));
                }
            }
            Ok(WalkControl::Continue)
        })?;

        Ok((visited, changes))
    }

    /// Streaming in-place transform without retaining per-byte undo deltas.
    ///
    /// Callers should only pair this with a compact undo record when the
    /// pre-edit range was checked with [`replacement_range_is_pristine`].
    pub fn transform_visible_range_in_place_compact(
        &mut self,
        start: u64,
        end_inclusive: u64,
        mut transform: impl FnMut(u8) -> u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        self.walk_visible_cells(
            start,
            end_inclusive,
            REPLACEMENT_CHUNK as usize,
            |document, chunk| {
                for cell in chunk.cells {
                    if cell.deleted {
                        continue;
                    }
                    stats.visited += 1;
                    let updated = transform(cell.byte);
                    let before = document.replacement_state(cell.id);
                    document.set_display_byte_by_id(cell.id, updated)?;
                    let after = document.replacement_state(cell.id);
                    if after != before {
                        stats.changed += 1;
                    }
                }
                Ok(WalkControl::Continue)
            },
        )?;

        Ok(stats)
    }

    /// Overwrite a run of consecutive display cells starting at `offset`,
    /// generating each cell's new byte from its zero-based position in the run
    /// via `byte_at`. Streams cell resolution in 64 KB batches so a multi-GB
    /// fill never resolves every `CellId` up front.
    ///
    /// Matches the overwrite-paste contract used by `:fill`/`:zero`: writes are
    /// clamped to the current display length (bytes past EOF are dropped), and
    /// hitting a tombstoned cell is an error (overwrite does not skip slots).
    /// Returns the run length actually written plus the per-cell replacement
    /// changes for undo.
    pub fn overwrite_run_positional(
        &mut self,
        offset: u64,
        run_len: u64,
        mut byte_at: impl FnMut(u64) -> u8,
    ) -> HxResult<(u64, Vec<ReplacementDelta>)> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if run_len == 0 || offset >= doc_len {
            return Ok((0, Vec::new()));
        }
        let applied = run_len.min(doc_len - offset);
        let mut changes = Vec::new();
        let mut written = 0_u64;

        while written < applied {
            let batch = (applied - written).min(REPLACEMENT_CHUNK);
            let ids = self.cell_ids_range(offset + written, batch);
            for (i, id) in ids.into_iter().enumerate() {
                if self.is_tombstone(id) {
                    return Err(HxError::OffsetOutOfRange);
                }
                let value = byte_at(written + i as u64);
                let before = self.replacement_state(id);
                self.set_display_byte_by_id(id, value)?;
                let after = self.replacement_state(id);
                if after != before {
                    changes.push((id, before, after));
                }
            }
            written += batch;
        }

        Ok((applied, changes))
    }

    /// Overwrite a display run without retaining per-byte undo deltas.
    ///
    /// Returns the number of cells written and the number of replacement states
    /// that changed. Callers are responsible for recording a compact undo op
    /// when appropriate.
    pub fn overwrite_run_positional_compact(
        &mut self,
        offset: u64,
        run_len: u64,
        mut byte_at: impl FnMut(u64) -> u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if run_len == 0 || offset >= doc_len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let applied = run_len.min(doc_len - offset);
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };

        while stats.visited < applied {
            let batch = (applied - stats.visited).min(REPLACEMENT_CHUNK);
            let ids = self.cell_ids_range(offset + stats.visited, batch);
            for (i, id) in ids.into_iter().enumerate() {
                if self.is_tombstone(id) {
                    return Err(HxError::OffsetOutOfRange);
                }
                let value = byte_at(stats.visited + i as u64);
                let before = self.replacement_state(id);
                self.set_display_byte_by_id(id, value)?;
                let after = self.replacement_state(id);
                if after != before {
                    stats.changed += 1;
                }
            }
            stats.visited += batch;
        }

        Ok(stats)
    }

    /// Clear replacement entries in a display range without changing piece
    /// layout or tombstones.
    pub fn clear_replacements_in_display_range(&mut self, offset: u64, len: u64) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if len == 0 {
            return Ok(());
        }
        let end = offset.checked_add(len).ok_or(HxError::OffsetOutOfRange)?;
        if end > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }

        let mut cleared = 0_u64;
        while cleared < len {
            let batch = (len - cleared).min(REPLACEMENT_CHUNK);
            for id in self.cell_ids_range(offset + cleared, batch) {
                self.replacements.remove(&id);
            }
            cleared += batch;
        }
        Ok(())
    }

    /// Re-apply a set of `(offset, bytes)` replacement spans onto this
    /// document. Pure replacement semantics: every byte must fall within the
    /// current display bounds, so this never inserts, tombstones, or
    /// real-deletes. The inverse of [`Document::replacement_spans`], used to
    /// restore per-region memory edits after rebuilding a fixed-size document.
    pub fn apply_replacement_spans(&mut self, spans: &[(u64, Vec<u8>)]) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        for (offset, bytes) in spans {
            for (index, value) in bytes.iter().enumerate() {
                let target = offset
                    .checked_add(index as u64)
                    .ok_or(HxError::OffsetOutOfRange)?;
                if target >= self.len() {
                    return Err(HxError::OffsetOutOfRange);
                }
                self.replace_display_byte(target, *value)?;
            }
        }
        Ok(())
    }

    /// Set a byte: replace if within bounds, insert if at EOF.
    pub fn set_byte(&mut self, offset: u64, value: u8) -> HxResult<()> {
        if offset == self.len() {
            if self.fixed_size {
                return Err(HxError::FixedSizeViolation);
            }
            self.insert_byte(offset, value)?;
            return Ok(());
        }
        self.replace_display_byte(offset, value)?;
        Ok(())
    }

    /// Insert a single byte at `offset`. Subsequent display offsets shift right.
    pub fn insert_byte(&mut self, offset: u64, value: u8) -> HxResult<CellId> {
        let inserted = self.insert_bytes(offset, &[value])?;
        inserted.first().copied().ok_or(HxError::OffsetOutOfRange)
    }

    /// Insert multiple bytes at `offset`. Returns the `CellId`s of the new bytes.
    pub fn insert_bytes(&mut self, offset: u64, bytes: &[u8]) -> HxResult<Vec<CellId>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if self.fixed_size {
            return Err(HxError::FixedSizeViolation);
        }
        if offset > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        if bytes.is_empty() {
            return Ok(Vec::new());
        }

        let add_start = self.pieces.add_len();
        self.pieces.insert_bytes(offset, bytes);
        Ok((0..bytes.len())
            .map(|idx| CellId::Add(add_start + idx as u64))
            .collect())
    }

    /// Tombstone-delete a byte (convenience wrapper over `mark_tombstone`).
    pub fn delete_byte(&mut self, offset: u64) -> HxResult<Option<CellId>> {
        self.mark_tombstone(offset)
    }

    /// Real-delete bytes from the piece table (insert-mode backspace).
    pub fn delete_range_real(&mut self, offset: u64, len: u64) -> HxResult<Vec<CellId>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if self.fixed_size {
            return Err(HxError::FixedSizeViolation);
        }
        if len == 0 {
            return Ok(Vec::new());
        }
        if offset >= self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok(self.pieces.delete_range_real(offset, len))
    }

    /// Re-insert previously removed cells (undo of real-delete).
    pub fn restore_real_delete(&mut self, offset: u64, cells: &[CellId]) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if self.fixed_size {
            return Err(HxError::FixedSizeViolation);
        }
        if offset > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        self.pieces.insert_existing_cells(offset, cells);
        Ok(())
    }
}
