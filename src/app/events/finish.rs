use super::*;

impl App {
    pub(super) fn finish_action(&mut self, action: Action, result: HxResult<()>) {
        match result {
            Ok(()) => {
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
                self.refresh_data_panel();
                if !is_command_edit_action(action) {
                    self.clear_error_if_command_done();
                }
                if self.diff_state().is_some_and(|state| state.stale) {
                    self.set_notice_status("diff stale; run :diff refresh");
                }
            }
            Err(err) => {
                self.set_error_status(err.to_string());
            }
        }
    }
}

fn is_command_edit_action(action: Action) -> bool {
    matches!(
        action,
        Action::CommandChar(_)
            | Action::CommandLeft
            | Action::CommandRight
            | Action::CommandHome
            | Action::CommandEnd
            | Action::CommandDelete
            | Action::CommandBackspace
            | Action::CommandHistoryPrev
            | Action::CommandHistoryNext
    )
}
