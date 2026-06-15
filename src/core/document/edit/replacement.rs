use super::*;

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

    pub(crate) fn display_range_has_replacement_range(&self, offset: u64, len: u64) -> bool {
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
                if self.has_replacement_range_in_source_range(
                    piece.source,
                    source_start,
                    overlap_end - overlap_start,
                ) {
                    return true;
                }
            }
            display_cursor = piece_end;
        }
        false
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
}
