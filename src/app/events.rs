use crate::action::Action;
use crate::app::text_cursor::{
    backspace_char_before_cursor, delete_char_at_cursor, insert_char_at_cursor, move_cursor_end,
    move_cursor_home, move_cursor_left, move_cursor_right,
};
use crate::app::{App, SidePanelKind};
use crate::error::{HxError, HxResult};
use crate::format::parse::InspectorRow;
use crate::mode::Mode;
use crate::mode::NibblePhase;

impl App {
    pub(crate) fn handle_action(&mut self, action: Action) {
        let result = self.dispatch_action(action);
        self.finish_action(action, result);
    }
}

mod dispatch;
mod editor;
mod finish;
mod inspector_edit;

#[cfg(test)]
mod tests;
