use crate::app::App;
use crate::error::HxResult;
use crate::mode::{Mode, NibblePhase};

impl App {
    pub(crate) fn delete_at_cursor_or_selection(&mut self) -> HxResult<()> {
        if matches!(self.mode, Mode::Visual) {
            return self.delete_selection();
        }
        self.delete_current()
    }

    pub(crate) fn delete_current(&mut self) -> HxResult<()> {
        let previous_patch = self.document.patch_state_at(self.cursor);
        self.document.delete_byte(self.cursor)?;
        self.push_undo_if_changed(self.cursor, previous_patch, self.cursor, self.mode);
        self.status_message = format!("deleted 0x{:x}", self.cursor);
        Ok(())
    }

    pub(crate) fn delete_selection(&mut self) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return self.delete_current();
        };

        let cursor_before = start;
        let mode_before = Mode::Visual;
        let mut undo_entries = Vec::with_capacity((end - start + 1) as usize);
        for offset in start..=end {
            let previous_patch = self.document.patch_state_at(offset);
            self.document.delete_byte(offset)?;
            if self.document.patch_state_at(offset) != previous_patch {
                undo_entries.push(crate::app::UndoEntry {
                    offset,
                    previous_patch,
                    cursor_before,
                    mode_before,
                });
            }
        }

        self.push_undo_step(undo_entries);
        self.cursor = self.clamp_offset(start);
        self.selection_anchor = None;
        self.mode = Mode::Normal;
        self.status_message = format!("deleted selection {} bytes", end - start + 1);
        Ok(())
    }

    pub(crate) fn edit_nibble(&mut self, value: u8) -> HxResult<()> {
        let offset = self.cursor;
        let phase = match self.mode {
            Mode::EditHex { phase } => phase,
            _ => return Ok(()),
        };
        let previous_patch = self.document.patch_state_at(offset);
        self.document.replace_nibble(offset, phase, value)?;
        self.push_undo_if_changed(offset, previous_patch, self.cursor, self.mode);
        self.status_message = format!("edited 0x{:x}", offset);
        self.mode = match phase {
            NibblePhase::High => Mode::EditHex {
                phase: NibblePhase::Low,
            },
            NibblePhase::Low => {
                let next = offset + 1;
                self.cursor = next.min(self.document.len());
                Mode::EditHex {
                    phase: NibblePhase::High,
                }
            }
        };
        Ok(())
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
            Mode::EditHex { .. } | Mode::Command => {}
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

    pub(crate) fn leave_mode(&mut self) {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Command => {
                self.mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
            }
            _ => {
                self.mode = Mode::Normal;
            }
        }
    }

    pub(crate) fn selection_range(&self) -> Option<(u64, u64)> {
        let anchor = self.selection_anchor?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }
}
