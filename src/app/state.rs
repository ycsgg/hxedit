use crate::app::{App, EditOp, ReplacementUndo};
use crate::error::HxResult;
use crate::mode::{Mode, NibblePhase, PendingInsert};

impl App {
    /// Delete the byte at cursor (normal) or the entire selection (visual).
    /// Both use tombstone deletion — the cell keeps its display slot.
    pub(crate) fn delete_at_cursor_or_selection(&mut self) -> HxResult<()> {
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
        );
        self.status_message = format!("deleted 0x{:x}", self.cursor);
        Ok(())
    }

    /// Tombstone-delete every byte in the visual selection.
    pub(crate) fn delete_selection(&mut self) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return self.delete_current();
        };

        let mut ids = Vec::with_capacity((end - start + 1) as usize);
        for offset in start..=end {
            if let Some(id) = self.document.delete_byte(offset)? {
                ids.push(id);
            }
        }

        self.push_undo_step(vec![EditOp::TombstoneDelete { ids }], start, Mode::Visual);
        self.cursor = self.clamp_offset(start);
        self.selection_anchor = None;
        self.mode = Mode::Normal;
        self.status_message = format!("deleted selection {} bytes", end - start + 1);
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
        if after != previous {
            self.push_undo_step(
                vec![EditOp::ReplaceBytes {
                    changes: vec![ReplacementUndo { id, previous }],
                }],
                self.cursor,
                self.mode,
            );
        } else if offset == self.document.len().saturating_sub(1) && previous.is_none() {
            self.push_undo_step(vec![EditOp::Insert { offset, len: 1 }], offset, self.mode);
        }

        self.status_message = format!("edited 0x{:x}", offset);
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
                self.document.insert_byte(offset, value << 4)?;
                // Keep cursor on the just-inserted byte so the user sees `a0`
                // and can type the second nibble.  Cursor advances only after
                // the low nibble is filled in (see the Some branch below).
                self.cursor = offset;
                self.mode = Mode::InsertHex {
                    pending: Some(PendingInsert {
                        offset,
                        high_nibble: value,
                    }),
                };
                self.status_message = format!("inserted 0x{:x}", offset);
            }
            Some(pending) => {
                self.document
                    .replace_display_byte(pending.offset, (pending.high_nibble << 4) | value)?;
                self.commit_pending_insert()?;
                // Now advance past the completed byte.
                self.cursor = pending.offset + 1;
                self.status_message = format!("inserted 0x{:x}", pending.offset);
            }
        }

        Ok(())
    }

    /// Backspace in insert mode — the only "real delete" interaction.
    ///
    /// - **With pending**: remove the just-inserted half-byte, no undo step
    ///   (the byte was never committed).
    /// - **Without pending**: real-delete the byte before cursor, push a
    ///   `RealDelete` undo step so it can be restored.
    pub(crate) fn edit_backspace(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::InsertHex { pending } => {
                if let Some(pending) = pending {
                    self.document.delete_range_real(pending.offset, 1)?;
                    self.cursor = pending.offset;
                    self.mode = Mode::InsertHex { pending: None };
                    self.status_message = format!("deleted 0x{:x}", pending.offset);
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
                );
                self.cursor = delete_offset;
                self.status_message = format!("deleted 0x{:x}", delete_offset);
                Ok(())
            }
            Mode::EditHex { .. } | Mode::Normal | Mode::Visual | Mode::Command => Ok(()),
        }
    }

    pub(crate) fn toggle_visual(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Normal => {
                self.selection_anchor = Some(self.cursor);
                self.mode = Mode::Visual;
            }
            Mode::EditHex { .. } | Mode::InsertHex { .. } | Mode::Command => {}
        }
        Ok(())
    }

    pub(crate) fn clear_error_if_command_done(&mut self) {
        let is_error = self.status_message.starts_with("invalid")
            || self.status_message.starts_with("unknown")
            || self.status_message.starts_with("missing")
            || self.status_message.contains("outside");
        if !matches!(self.mode, Mode::Command) && is_error {
            self.status_message.clear();
        }
    }

    /// Leave the current mode (Esc handler).
    ///
    /// - Visual → Normal (clear selection)
    /// - Command → return mode
    /// - InsertHex → commit pending, then Normal
    /// - EditHex / Normal → Normal
    pub(crate) fn leave_mode(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Command => {
                self.mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
            }
            Mode::InsertHex { .. } => {
                self.commit_pending_insert()?;
                self.mode = Mode::Normal;
            }
            Mode::EditHex { .. } | Mode::Normal => {
                self.mode = Mode::Normal;
            }
        }
        Ok(())
    }

    /// Finalize a half-completed insert byte into the undo stack.
    ///
    /// Called when leaving insert mode, moving the cursor, entering command
    /// mode, or clicking the mouse.  The pending byte's low nibble `0` is
    /// fixated as a real value.
    pub(crate) fn commit_pending_insert(&mut self) -> HxResult<Option<u64>> {
        let pending = match self.mode {
            Mode::InsertHex {
                pending: Some(pending),
            } => pending,
            _ => return Ok(None),
        };

        self.push_undo_step(
            vec![EditOp::Insert {
                offset: pending.offset,
                len: 1,
            }],
            pending.offset,
            Mode::InsertHex { pending: None },
        );
        self.mode = Mode::InsertHex { pending: None };
        Ok(Some(pending.offset))
    }

    /// Undo a pending insert without pushing an undo step.
    ///
    /// Used when the user presses undo while a half-byte is pending: the
    /// byte is silently removed (it was never committed to the undo stack).
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
        self.status_message = "undid 1 action".to_owned();
        Ok(true)
    }

    /// Commit any pending insert before a cursor move or mode switch.
    pub(crate) fn ensure_insert_pending_committed(&mut self) -> HxResult<()> {
        if matches!(self.mode, Mode::InsertHex { .. }) {
            self.commit_pending_insert()?;
        }
        Ok(())
    }

    pub(crate) fn selection_range(&self) -> Option<(u64, u64)> {
        let anchor = self.selection_anchor?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }
}
