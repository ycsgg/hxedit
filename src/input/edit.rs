use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::input::keymap::movement_action;
use crate::util::parse::parse_hex_nibble;

pub fn map(key: KeyEvent) -> Option<Action> {
    if matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z'))
        && (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::SUPER))
    {
        return Some(Action::Undo(1));
    }
    if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y'))
        && (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::SUPER))
    {
        return Some(Action::Redo(1));
    }
    if let Some(action) = movement_action(&key.code) {
        return Some(action);
    }

    match key.code {
        KeyCode::Esc => Some(Action::LeaveMode),
        KeyCode::Backspace => Some(Action::EditBackspace),
        KeyCode::Char(c) => parse_hex_nibble(c).map(Action::EditHex),
        _ => None,
    }
}
