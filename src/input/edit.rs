use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::util::parse::parse_hex_nibble;

pub fn map(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::LeaveMode),
        KeyCode::Left | KeyCode::Char('h') => Some(Action::MoveLeft),
        KeyCode::Right | KeyCode::Char('l') => Some(Action::MoveRight),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home => Some(Action::RowStart),
        KeyCode::End => Some(Action::RowEnd),
        KeyCode::Char(c) => parse_hex_nibble(c).map(Action::EditHex),
        _ => None,
    }
}
