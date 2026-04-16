use crate::core::document::Document;
use crate::core::piece_table::CellId;
use crate::error::{HxError, HxResult};
use crate::mode::NibblePhase;

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

    /// Tombstone-delete a byte (normal/visual mode). The cell keeps its
    /// display slot but renders as `Deleted` and is skipped on save.
    pub fn mark_tombstone(&mut self, offset: u64) -> HxResult<Option<CellId>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
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

    /// Set a byte: replace if within bounds, insert if at EOF.
    pub fn set_byte(&mut self, offset: u64, value: u8) -> HxResult<()> {
        if offset == self.len() {
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
        if offset > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        self.pieces.insert_existing_cells(offset, cells);
        Ok(())
    }
}
