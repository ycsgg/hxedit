use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;

pub fn map(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::CommandCancel),
        KeyCode::Enter => Some(Action::CommandSubmit),
        KeyCode::Backspace => Some(Action::CommandBackspace),
        KeyCode::Char(c) => Some(Action::CommandChar(c)),
        _ => None,
    }
}
