use std::sync::Arc;

use crate::core::document::{BytesOverlayRun, Document, ReplacementPatch};
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
    pub fn replacement_state(&mut self, id: CellId) -> HxResult<Option<u8>> {
        let base = self.base_byte(id)?;
        Ok(self.replacements.get(id, base))
    }

    /// Restore a replacement to its previous state (used by undo).
    pub fn restore_replacement(&mut self, id: CellId, previous: Option<u8>) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        match previous {
            Some(value) => {
                self.replacements.set_cell(id, value);
            }
            None => {
                self.replacements.clear_cell(id);
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

    pub(crate) fn replacement_patch_for_display_range(
        &self,
        offset: u64,
        len: u64,
    ) -> HxResult<ReplacementPatch> {
        if len == 0 {
            return Ok(ReplacementPatch::default());
        }
        let end = offset.checked_add(len).ok_or(HxError::OffsetOutOfRange)?;
        if end > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }

        let mut patch = ReplacementPatch::default();
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
                let source_start = piece.start + (overlap_start - display_cursor);
                patch.extend(self.replacements.patch_for_source_range(
                    piece.source,
                    source_start,
                    overlap_end - overlap_start,
                ));
            }
            display_cursor = piece_end;
        }
        Ok(patch)
    }

    pub(crate) fn restore_replacement_patch_in_display_range(
        &mut self,
        offset: u64,
        len: u64,
        patch: &ReplacementPatch,
    ) -> HxResult<()> {
        self.clear_replacements_in_display_range(offset, len)?;
        self.replacements.apply_patch(patch);
        Ok(())
    }

    pub(crate) fn display_range_has_tombstone(&self, offset: u64, len: u64) -> bool {
        if len == 0 || offset >= self.len() || !self.has_tombstones() {
            return false;
        }
        let end = offset.saturating_add(len).min(self.len());
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
                let source_start = piece.start + (overlap_start - display_cursor);
                let lo = CellId::from_source(piece.source, source_start);
                let hi = CellId::from_source(
                    piece.source,
                    source_start + overlap_end - overlap_start - 1,
                );
                if self.has_tombstone_in_range(lo, hi) {
                    return true;
                }
            }
            display_cursor = piece_end;
        }
        false
    }

    pub(crate) fn display_range_has_sparse_replacement(&self, offset: u64, len: u64) -> bool {
        if len == 0 || offset >= self.len() || !self.has_replacements() {
            return false;
        }
        let end = offset.saturating_add(len).min(self.len());
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
                let source_start = piece.start + (overlap_start - display_cursor);
                let lo = CellId::from_source(piece.source, source_start);
                let hi = CellId::from_source(
                    piece.source,
                    source_start + overlap_end - overlap_start - 1,
                );
                if self.replacements.has_sparse_in_range(lo, hi) {
                    return true;
                }
            }
            display_cursor = piece_end;
        }
        false
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
                let before = document.replacement_state(cell.id)?;
                document.set_display_byte_by_id(cell.id, updated)?;
                let after = document.replacement_state(cell.id)?;
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
                    let before = document.replacement_state(cell.id)?;
                    document.set_display_byte_by_id(cell.id, updated)?;
                    let after = document.replacement_state(cell.id)?;
                    if after != before {
                        stats.changed += 1;
                    }
                }
                Ok(WalkControl::Continue)
            },
        )?;

        Ok(stats)
    }

    /// Apply an XOR overlay over a clean display range without expanding one
    /// replacement entry per byte. Callers must only use this when the range
    /// has no existing tombstones/replacements.
    pub fn xor_visible_range_overlay(
        &mut self,
        start: u64,
        end_inclusive: u64,
        key: u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if key == 0 || len == 0 || start > end_inclusive || start >= len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }
        let end = end_inclusive.min(len - 1) + 1;
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };

        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let range_len = overlap_end - overlap_start;
                self.replacements
                    .set_xor_range(piece.source, source_start, range_len, key);
                stats.visited += range_len;
                stats.changed += range_len;
            }
            display_cursor = piece_end;
        }

        Ok(stats)
    }

    pub fn xor_visible_range_bytes_overlay_changed(
        &mut self,
        start: u64,
        end_inclusive: u64,
        key: u8,
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
                let mut run_start = 0_u64;
                let mut run_bytes = Vec::new();

                for cell in chunk.cells {
                    if cell.deleted {
                        document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                        continue;
                    }

                    stats.visited += 1;
                    let updated = cell.byte ^ key;
                    if updated == cell.byte {
                        document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                        continue;
                    }

                    stats.changed += 1;
                    if run_bytes.is_empty() {
                        run_start = cell.display_offset;
                    } else if run_start + run_bytes.len() as u64 != cell.display_offset {
                        document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                        run_start = cell.display_offset;
                    }
                    run_bytes.push(updated);
                }

                document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                Ok(WalkControl::Continue)
            },
        )?;

        Ok(stats)
    }

    pub fn xor_visible_range_mixed_overlay(
        &mut self,
        start: u64,
        end_inclusive: u64,
        key: u8,
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

        let end = end_inclusive.min(len - 1) + 1;
        let range_len = end - start;
        if self.display_range_has_tombstone(start, range_len)
            || self.display_range_has_sparse_replacement(start, range_len)
        {
            return self.xor_visible_range_bytes_overlay_changed(start, end - 1, key);
        }

        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let segment_len = overlap_end - overlap_start;
                self.replacements.xor_source_range_composed(
                    piece.source,
                    source_start,
                    segment_len,
                    key,
                );
                stats.visited += segment_len;
                if key != 0 {
                    stats.changed += segment_len;
                }
            }
            display_cursor = piece_end;
        }

        Ok(stats)
    }

    fn flush_bytes_overlay_run(&mut self, run_start: u64, run_bytes: &mut Vec<u8>) -> HxResult<()> {
        if run_bytes.is_empty() {
            return Ok(());
        }
        let bytes: Arc<[u8]> = Arc::from(std::mem::take(run_bytes));
        self.overwrite_run_bytes_overlay(run_start, bytes)?;
        Ok(())
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
                let before = self.replacement_state(id)?;
                self.set_display_byte_by_id(id, value)?;
                let after = self.replacement_state(id)?;
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
                let before = self.replacement_state(id)?;
                self.set_display_byte_by_id(id, value)?;
                let after = self.replacement_state(id)?;
                if after != before {
                    stats.changed += 1;
                }
            }
            stats.visited += batch;
        }

        Ok(stats)
    }

    /// Overwrite a display run with a repeating pattern using range overlays.
    ///
    /// This is the large clean-range fast path for `:fill` / `:zero`: the
    /// command's overwrite intent marks every covered cell changed, so the
    /// fast path can install one range per contiguous piece overlap without
    /// scanning base bytes to detect no-op positions.
    pub fn overwrite_run_pattern_overlay(
        &mut self,
        offset: u64,
        run_len: u64,
        pattern: &[u8],
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if pattern.is_empty() || run_len == 0 || offset >= doc_len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let applied = run_len.min(doc_len - offset);
        let end = offset + applied;
        let pattern: Arc<[u8]> = Arc::from(pattern);
        let pattern_len = pattern.len() as u64;
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        let mut segments = Vec::new();

        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= offset {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = offset.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let range_len = overlap_end - overlap_start;
                let phase = (overlap_start - offset) % pattern_len;
                stats.visited += range_len;
                stats.changed += range_len;
                segments.push((piece.source, source_start, range_len, phase));
            }
            display_cursor = piece_end;
        }

        if stats.visited > 0 {
            for (source, source_start, range_len, phase) in segments {
                self.replacements.set_pattern_range(
                    source,
                    source_start,
                    range_len,
                    Arc::clone(&pattern),
                    phase,
                );
            }
        }

        Ok(stats)
    }

    /// Overwrite a display run with explicit bytes using range overlays.
    ///
    /// This is the compact replacement primitive for clean overwrite-paste
    /// runs. The bytes are not repeated; each covered display cell takes the
    /// corresponding byte from `bytes`.
    pub fn overwrite_run_bytes_overlay(
        &mut self,
        offset: u64,
        bytes: Arc<[u8]>,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if bytes.is_empty() || offset >= doc_len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let applied = (bytes.len() as u64).min(doc_len - offset);
        let end = offset + applied;
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        let mut segments = Vec::new();

        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= offset {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = offset.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let range_len = overlap_end - overlap_start;
                let phase = overlap_start - offset;
                stats.visited += range_len;
                stats.changed += range_len;
                segments.push((piece.source, source_start, range_len, phase));
            }
            display_cursor = piece_end;
        }

        if stats.visited > 0 {
            for (source, source_start, range_len, phase) in segments {
                self.replacements.set_bytes_range(
                    source,
                    source_start,
                    range_len,
                    Arc::clone(&bytes),
                    phase,
                );
            }
        }

        Ok(stats)
    }

    /// Apply bytes overlays only for cells whose current visible byte differs
    /// from the pasted byte. Returns the clamped write length plus compact
    /// `(display_offset, bytes)` runs suitable for undo/redo records.
    pub fn overwrite_run_bytes_overlay_changed(
        &mut self,
        offset: u64,
        bytes: &[u8],
    ) -> HxResult<(u64, Vec<BytesOverlayRun>)> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if bytes.is_empty() || offset >= doc_len {
            return Ok((0, Vec::new()));
        }

        let applied = bytes.len().min((doc_len - offset) as usize);
        let end_inclusive = offset + applied as u64 - 1;
        let mut runs: Vec<BytesOverlayRun> = Vec::new();
        let mut run_start = 0_u64;
        let mut run_bytes = Vec::new();

        self.walk_visible_cells(
            offset,
            end_inclusive,
            REPLACEMENT_CHUNK as usize,
            |_, chunk| {
                for cell in chunk.cells {
                    if cell.deleted {
                        return Err(HxError::OffsetOutOfRange);
                    }
                    let index = (cell.display_offset - offset) as usize;
                    let value = bytes[index];
                    if cell.byte != value {
                        if run_bytes.is_empty() {
                            run_start = cell.display_offset;
                        } else if run_start + run_bytes.len() as u64 != cell.display_offset {
                            let bytes: Arc<[u8]> = Arc::from(std::mem::take(&mut run_bytes));
                            runs.push((run_start, bytes));
                            run_start = cell.display_offset;
                        }
                        run_bytes.push(value);
                    } else if !run_bytes.is_empty() {
                        let bytes: Arc<[u8]> = Arc::from(std::mem::take(&mut run_bytes));
                        runs.push((run_start, bytes));
                    }
                }
                Ok(WalkControl::Continue)
            },
        )?;

        if !run_bytes.is_empty() {
            let bytes: Arc<[u8]> = Arc::from(run_bytes);
            runs.push((run_start, bytes));
        }

        for (run_offset, run_bytes) in &runs {
            self.overwrite_run_bytes_overlay(*run_offset, Arc::clone(run_bytes))?;
        }

        Ok((applied as u64, runs))
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
            self.clear_replacement_display_subrange(offset + cleared, batch)?;
            cleared += batch;
        }
        Ok(())
    }

    fn clear_replacement_display_subrange(&mut self, offset: u64, len: u64) -> HxResult<()> {
        let end = offset + len;
        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= offset {
                display_cursor = piece_end;
                continue;
            }
            let overlap_start = offset.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                self.replacements.clear_source_range(
                    piece.source,
                    source_start,
                    overlap_end - overlap_start,
                );
            }
            display_cursor = piece_end;
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
