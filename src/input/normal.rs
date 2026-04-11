use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;

pub fn map(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => Some(Action::MoveLeft),
        KeyCode::Right | KeyCode::Char('l') => Some(Action::MoveRight),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home => Some(Action::RowStart),
        KeyCode::End => Some(Action::RowEnd),
        KeyCode::Char('i') | KeyCode::Char('r') => Some(Action::EnterEdit),
        KeyCode::Char('x') => Some(Action::DeleteByte),
        KeyCode::Char('n') => Some(Action::SearchNext),
        KeyCode::Char('p') => Some(Action::SearchPrev),
        KeyCode::Char(':') => Some(Action::EnterCommand),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::ForceQuit)
        }
        _ => None,
    }
}
