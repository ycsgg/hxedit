use super::*;

impl App {
    pub(super) fn execute_quit_command(&mut self, force: bool) -> HxResult<()> {
        if self.document.is_dirty() && !force {
            return Err(HxError::DirtyQuit);
        }
        self.should_quit = true;
        Ok(())
    }

    pub(super) fn execute_write_command(
        &mut self,
        path: Option<std::path::PathBuf>,
        should_quit: bool,
    ) -> HxResult<()> {
        let (saved, profile) = self.document.save(path)?;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.mark_document_changed();
        self.cursor = self.clamp_offset(self.cursor);
        self.invalidate_disassembly_cache();
        self.refresh_inspector();
        self.set_info_status(format!("wrote {} [{}]", saved.display(), profile));
        self.should_quit = should_quit;
        Ok(())
    }

    pub(super) fn execute_goto_command(&mut self, target: GotoTarget) -> HxResult<()> {
        let cursor_before = self.cursor;
        let offset = self.resolve_goto_target(target)?;
        self.cursor = self.document.goto(offset)?;
        self.set_info_status(format!(
            "moved {} → 0x{:x}",
            format_move_delta(cursor_before, self.cursor),
            self.cursor
        ));
        Ok(())
    }

    fn resolve_goto_target(&self, target: GotoTarget) -> HxResult<u64> {
        match target {
            GotoTarget::Absolute(offset) => Ok(offset),
            GotoTarget::End => {
                if self.document.is_empty() {
                    Ok(0)
                } else {
                    Ok(self.document.len() - 1)
                }
            }
            GotoTarget::Relative(delta) => {
                let current = i64::try_from(self.cursor)
                    .map_err(|_| HxError::InvalidOffset(delta.to_string()))?;
                let target = current.saturating_add(delta);
                u64::try_from(target).map_err(|_| HxError::OffsetOutOfRange)
            }
        }
    }
}

fn format_move_delta(before: u64, after: u64) -> String {
    if after >= before {
        format!("+0x{:x}", after - before)
    } else {
        format!("-0x{:x}", before - after)
    }
}
