use super::*;

impl App {
    pub(super) fn execute_hash_command(&mut self, algorithm: HashAlgorithm) -> HxResult<()> {
        let selection = self.active_selection_range();
        let (start, end, scope) = if let Some((start, end)) = selection {
            (start, end, crate::app::hash_state::HashScope::Selection)
        } else if self.document.is_empty() {
            self.set_info_status(format!("{}: no data to hash", algorithm.label()));
            return Ok(());
        } else {
            (
                0,
                self.document.len() - 1,
                crate::app::hash_state::HashScope::EntireFile,
            )
        };

        let display_total = end.saturating_sub(start).saturating_add(1);
        if display_total > crate::app::hash_state::HASH_PROGRESS_STEP_BYTES {
            self.start_hash_scan(algorithm, scope, start, end);
            return Ok(());
        }

        let hash = crate::exec::hash_display_range(&mut self.document, algorithm, start, end)?;
        let bytes_hashed = hash.bytes_hashed;

        if bytes_hashed == 0 {
            self.set_info_status(format!("{}: no data to hash", algorithm.label()));
            return Ok(());
        }

        self.set_hash_result_status(algorithm, scope.label(start, end), hash.hex, bytes_hashed);
        Ok(())
    }

    pub(super) fn execute_diff_command(&mut self, command: DiffCommand) -> HxResult<()> {
        match command {
            DiffCommand::Open { path, max_shift } => self.open_diff_panel(path, max_shift),
            DiffCommand::Refresh => self.refresh_diff_panel(),
            DiffCommand::Next => self.jump_to_next_diff_mismatch(),
            DiffCommand::Prev => self.jump_to_prev_diff_mismatch(),
            DiffCommand::Off => {
                self.close_diff_panel();
                Ok(())
            }
        }
    }

    pub(crate) fn close_diff_projection_for_side_panel_switch(&mut self) {
        if self.diff_state().is_some() {
            self.diff_state = None;
            self.clear_diff_cell_selection();
        }
    }
}
