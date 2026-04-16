use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::input::keymap::{force_quit_action, movement_action};

pub fn map(key: KeyEvent) -> Option<Action> {
    if let Some(action) = force_quit_action(&key) {
        return Some(action);
    }
    if let Some(action) = movement_action(&key.code) {
        return Some(action);
    }

    match key.code {
        KeyCode::Esc => Some(Action::LeaveMode),
        KeyCode::Char('v') => Some(Action::ToggleVisual),
        KeyCode::Char('x') => Some(Action::DeleteByte),
        KeyCode::Char('n') => Some(Action::SearchNext),
        KeyCode::Char('p') => Some(Action::SearchPrev),
        KeyCode::Char(':') => Some(Action::EnterCommand),
        _ => None,
    }
}
