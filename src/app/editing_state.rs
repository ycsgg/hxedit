use crate::app::{App, EditOp, ReplacementChange};
use crate::error::HxResult;
use crate::mode::{Mode, NibblePhase, PendingInsert};

impl App {
    /// Delete the byte at cursor (normal) or the entire selection (visual).
    /// Both use tombstone deletion — the cell keeps its display slot.
    pub(crate) fn delete_at_cursor_or_selection(&mut self) -> HxResult<()> {
        if matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            return Err(crate::error::HxError::DisassemblyUnavailable(
                "view is overwrite-only; use :dis off for layout-changing edits".to_owned(),
            ));
        }
        if matches!(self.mode, Mode::Visual) {
            return self.delete_selection();
        }
        self.delete_current()
    }

    /// Tombstone-delete the single byte at cursor.
    pub(crate) fn delete_current(&mut self) -> HxResult<()> {
        let Some(id) = self.document.delete_byte(self.cursor)? else {
            return Ok(());
        };
        self.push_undo_step(
            vec![EditOp::TombstoneDelete { ids: vec![id] }],
            self.cursor,
            self.mode,
            self.cursor,
            self.mode,
        );
        self.refresh_inspector();
        self.set_info_status(format!("deleted 0x{:x}", self.cursor));
        Ok(())
    }

    /// Tombstone-delete every byte in the visual selection.
    pub(crate) fn delete_selection(&mut self) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return self.delete_current();
        };

        let span = end - start + 1;
        let candidates = self.document.cell_ids_range(start, span);
        let mut ids = Vec::with_capacity(candidates.len());
        for id in candidates {
            if self.document.is_tombstone(id) {
                continue;
            }
            self.document.mark_tombstones(&[id])?;
            ids.push(id);
        }

        self.cursor = self.clamp_offset(start);
        self.selection_anchor = None;
        self.mode = Mode::Normal;
        self.push_undo_step(
            vec![EditOp::TombstoneDelete { ids }],
            start,
            Mode::Visual,
            self.cursor,
            self.mode,
        );
        self.refresh_inspector();
        self.set_info_status(format!("deleted selection {} bytes", span));
        Ok(())
    }

    /// Process a hex nibble in edit (replace) mode.
    ///
    /// High nibble → sets the upper 4 bits, advances phase to Low.
    /// Low nibble  → sets the lower 4 bits, advances cursor to next byte,
    ///               resets phase to High.
    pub(crate) fn edit_nibble(&mut self, value: u8) -> HxResult<()> {
        let offset = self.cursor;
        let phase = match self.mode {
            Mode::EditHex { phase } => phase,
            _ => return Ok(()),
        };

        let previous = if offset < self.document.len() {
            self.document
                .cell_id_at(offset)
                .map(|id| (id, self.document.replacement_state(id)))
        } else {
            None
        };
        let id = self.document.replace_nibble(offset, phase, value)?;

        let after = self.document.replacement_state(id);
        let previous = previous
            .map(|(_, state)| state)
            .or_else(|| (offset < self.document.len()).then_some(None))
            .flatten();
        let cursor_before = self.cursor;
        let mode_before = self.mode;
        self.set_info_status(format!("edited 0x{:x}", offset));
        self.mode = match phase {
            NibblePhase::High => Mode::EditHex {
                phase: NibblePhase::Low,
            },
            NibblePhase::Low => {
                self.cursor = (offset + 1).min(self.document.len());
                Mode::EditHex {
                    phase: NibblePhase::High,
                }
            }
        };
        if after != previous {
            self.push_undo_step(
                vec![EditOp::ReplaceBytes {
                    changes: vec![ReplacementChange {
                        id,
                        before: previous,
                        after,
                    }],
                }],
                cursor_before,
                mode_before,
                self.cursor,
                self.mode,
            );
        } else if offset == self.document.len().saturating_sub(1) && previous.is_none() {
            self.push_undo_step(
                vec![EditOp::Insert {
                    offset,
                    cells: vec![id],
                }],
                cursor_before,
                mode_before,
                self.cursor,
                self.mode,
            );
        }
        self.refresh_inspector();
        Ok(())
    }

    /// Process a hex nibble in insert mode.
    ///
    /// Two-keystroke protocol:
    /// - **No pending**: insert a new byte `0xN0` at cursor, set pending.
    ///   Cursor stays on the just-inserted byte so the user sees `a0`.
    /// - **Has pending**: fill in the low nibble (`a0` → `ab`), commit the
    ///   insert to the undo stack, advance cursor past the completed byte.
    pub(crate) fn insert_nibble(&mut self, value: u8) -> HxResult<()> {
        let pending = match self.mode {
            Mode::InsertHex { pending } => pending,
            _ => return Ok(()),
        };

        match pending {
            None => {
                let offset = self.cursor;
                self.redo_stack.clear();
                self.document.insert_byte(offset, value << 4)?;
                self.cursor = offset;
                self.mode = Mode::InsertHex {
                    pending: Some(PendingInsert {
                        offset,
                        high_nibble: value,
                    }),
                };
                self.refresh_inspector();
                self.set_info_status(format!("inserted 0x{:x}", offset));
            }
            Some(pending) => {
                self.document
                    .replace_display_byte(pending.offset, (pending.high_nibble << 4) | value)?;
                self.cursor = pending.offset + 1;
                self.commit_pending_insert();
                self.refresh_inspector();
                self.set_info_status(format!("inserted 0x{:x}", pending.offset));
            }
        }

        Ok(())
    }

    /// Backspace in insert mode — the only "real delete" interaction.
    pub(crate) fn edit_backspace(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::InsertHex { pending } => {
                if let Some(pending) = pending {
                    self.redo_stack.clear();
                    self.document.delete_range_real(pending.offset, 1)?;
                    self.cursor = pending.offset;
                    self.mode = Mode::InsertHex { pending: None };
                    self.refresh_inspector();
                    self.set_info_status(format!("deleted 0x{:x}", pending.offset));
                    return Ok(());
                }

                if self.cursor == 0 {
                    return Ok(());
                }

                let delete_offset = self.cursor - 1;
                let removed = self.document.delete_range_real(delete_offset, 1)?;
                self.push_undo_step(
                    vec![EditOp::RealDelete {
                        offset: delete_offset,
                        cells: removed,
                    }],
                    self.cursor,
                    Mode::InsertHex { pending: None },
                    delete_offset,
                    Mode::InsertHex { pending: None },
                );
                self.cursor = delete_offset;
                self.refresh_inspector();
                self.set_info_status(format!("deleted 0x{:x}", delete_offset));
                Ok(())
            }
            Mode::EditHex { .. }
            | Mode::Normal
            | Mode::Visual
            | Mode::Command
            | Mode::Inspector
            | Mode::InspectorEdit => Ok(()),
        }
    }

    /// Finalize a half-completed insert byte into the undo stack.
    pub(crate) fn commit_pending_insert(&mut self) -> Option<u64> {
        let pending = match self.mode {
            Mode::InsertHex {
                pending: Some(pending),
            } => pending,
            _ => return None,
        };

        let id = self.document.cell_id_at(pending.offset)?;
        let cursor_after = self.cursor;
        let mode_after = Mode::InsertHex { pending: None };

        self.push_undo_step(
            vec![EditOp::Insert {
                offset: pending.offset,
                cells: vec![id],
            }],
            pending.offset,
            Mode::InsertHex { pending: None },
            cursor_after,
            mode_after,
        );
        self.mode = mode_after;
        Some(pending.offset)
    }

    /// Undo a pending insert without pushing an undo step.
    pub(crate) fn undo_pending_insert(&mut self) -> HxResult<bool> {
        let pending = match self.mode {
            Mode::InsertHex {
                pending: Some(pending),
            } => pending,
            _ => return Ok(false),
        };

        self.document.delete_range_real(pending.offset, 1)?;
        self.cursor = pending.offset;
        self.mode = Mode::InsertHex { pending: None };
        self.set_info_status("undid 1 action");
        Ok(true)
    }

    /// Commit any pending insert before a cursor move or mode switch.
    pub(crate) fn ensure_insert_pending_committed(&mut self) {
        if matches!(self.mode, Mode::InsertHex { .. }) {
            self.commit_pending_insert();
        }
    }
}
