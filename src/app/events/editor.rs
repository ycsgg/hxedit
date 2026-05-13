use super::*;

impl App {
    pub(super) fn enter_hex_mode(&mut self, insert: bool) -> HxResult<()> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        if insert && matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            return Err(HxError::DisassemblyUnavailable(
                "view is overwrite-only; use :dis off for layout-changing edits".to_owned(),
            ));
        }
        self.mode = if insert {
            Mode::InsertHex { pending: None }
        } else {
            Mode::EditHex {
                phase: NibblePhase::High,
            }
        };
        Ok(())
    }

    pub(super) fn enter_command_mode(&mut self) {
        let return_mode = if matches!(self.mode, Mode::InsertHex { .. }) {
            self.commit_pending_insert();
            Mode::Normal
        } else if matches!(self.mode, Mode::DisasmEdit) {
            self.cancel_disasm_edit();
            Mode::Normal
        } else {
            self.mode
        };
        self.command_return_mode = Some(return_mode);
        self.mode = Mode::Command;
        self.reset_command_history_navigation();
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
    }

    pub(super) fn handle_undo_action(&mut self, steps: usize) -> HxResult<()> {
        if self.undo_pending_insert()? {
            if steps > 1 {
                self.undo(steps - 1, true)
            } else {
                Ok(())
            }
        } else {
            self.undo(steps, true)
        }
    }

    pub(super) fn handle_redo_action(&mut self, steps: usize) -> HxResult<()> {
        self.redo(steps, true)
    }

    pub(super) fn handle_edit_hex_action(&mut self, value: u8) -> HxResult<()> {
        match self.mode {
            Mode::InsertHex { .. } => self.insert_nibble(value),
            _ => self.edit_nibble(value),
        }
    }
}
