use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;

pub fn map(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::CommandCancel),
        KeyCode::Enter => Some(Action::CommandSubmit),
        KeyCode::Left => Some(Action::CommandLeft),
        KeyCode::Right => Some(Action::CommandRight),
        KeyCode::Home => Some(Action::CommandHome),
        KeyCode::End => Some(Action::CommandEnd),
        KeyCode::Delete => Some(Action::CommandDelete),
        KeyCode::Backspace => Some(Action::CommandBackspace),
        KeyCode::Char(c) => Some(Action::CommandChar(c)),
        _ => None,
    }
}
