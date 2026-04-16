use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::input::keymap::{force_quit_action, movement_action};

pub fn map(key: KeyEvent) -> Option<Action> {
    if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y'))
        && (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::SUPER))
    {
        return Some(Action::Redo(1));
    }
    if let Some(action) = force_quit_action(&key) {
        return Some(action);
    }
    if let Some(action) = movement_action(&key.code) {
        return Some(action);
    }

    match key.code {
        KeyCode::Char('v') => Some(Action::ToggleVisual),
        KeyCode::Char('i') => Some(Action::EnterInsert),
        KeyCode::Char('r') => Some(Action::EnterReplace),
        KeyCode::Char('x') => Some(Action::DeleteByte),
        KeyCode::Char('n') => Some(Action::SearchNext),
        KeyCode::Char('p') => Some(Action::SearchPrev),
        KeyCode::Char('t') => Some(Action::ToggleInspector),
        KeyCode::Char(':') => Some(Action::EnterCommand),
        KeyCode::Tab => Some(Action::ToggleInspector),
        _ => None,
    }
}
