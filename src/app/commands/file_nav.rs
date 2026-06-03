use super::*;

impl App {
    pub(super) fn execute_quit_command(&mut self, force: bool) -> HxResult<()> {
        if !force {
            #[cfg(feature = "memory")]
            if let Some((regions, bytes)) = self.memory_dirty_summary() {
                return Err(HxError::MemoryDirtyQuit { regions, bytes });
            }
            if self.document.is_dirty() {
                return Err(HxError::DirtyQuit);
            }
        }
        self.should_quit = true;
        Ok(())
    }

    pub(super) fn execute_write_command(
        &mut self,
        path: Option<std::path::PathBuf>,
        should_quit: bool,
    ) -> HxResult<()> {
        // In memory mode, `:w` is equivalent to `:mem commit`. `:w <path>`
        // must not save process memory as a file; direct the user to :export.
        #[cfg(feature = "memory")]
        if self.document.is_fixed_size() {
            if path.is_some() {
                return Err(HxError::MemoryWritePath);
            }
            self.commit_memory_document(false)?;
            if should_quit {
                return self.execute_quit_command(false);
            }
            return Ok(());
        }
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
        let destination = self.display_offset_to_va(self.cursor).map_or_else(
            || format!("0x{:x}", self.cursor),
            |va| format!("VA 0x{va:x}"),
        );
        self.set_info_status(format!(
            "moved {} → {}",
            format_move_delta(cursor_before, self.cursor),
            destination
        ));
        Ok(())
    }

    fn resolve_goto_target(&self, target: GotoTarget) -> HxResult<u64> {
        match target {
            GotoTarget::Absolute(offset) => {
                if let Some(base_va) = self.memory_base_va() {
                    let end_va = base_va.saturating_add(self.document.len());
                    if base_va <= offset && offset < end_va {
                        return Ok(offset - base_va);
                    }
                }
                Ok(offset)
            }
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
