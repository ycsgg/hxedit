use super::*;

impl App {
    pub(super) fn execute_hash_command(&mut self, algorithm: HashAlgorithm) -> HxResult<()> {
        let selection = self.active_selection_range();
        let (start, end) = if let Some((start, end)) = selection {
            (start, end)
        } else if self.document.is_empty() {
            self.set_info_status(format!("{}: no data to hash", algorithm.label()));
            return Ok(());
        } else {
            (0, self.document.len() - 1)
        };

        let hash = crate::exec::hash_display_range(&mut self.document, algorithm, start, end)?;
        let bytes_hashed = hash.bytes_hashed;

        if bytes_hashed == 0 {
            self.set_info_status(format!("{}: no data to hash", algorithm.label()));
            return Ok(());
        }

        let scope = if selection.is_some() {
            format!("sel 0x{:x}-0x{:x}", start, end)
        } else {
            "entire file".to_owned()
        };

        if crate::clipboard::copy_text(&hash.hex).is_ok() {
            self.set_info_status(format!(
                "{} [{}]: {} ({} bytes) [copied]",
                algorithm.label(),
                scope,
                hash.hex,
                bytes_hashed
            ));
        } else {
            self.set_info_status(format!(
                "{} [{}]: {} ({} bytes)",
                algorithm.label(),
                scope,
                hash.hex,
                bytes_hashed
            ));
        }
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
