use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::input::keymap::force_quit_action;

pub fn map(key: KeyEvent) -> Option<Action> {
    if let Some(action) = force_quit_action(&key) {
        return Some(action);
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::InspectorUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::InspectorDown),
        KeyCode::Left => Some(Action::InspectorLeft),
        KeyCode::Right => Some(Action::InspectorRight),
        KeyCode::Home => Some(Action::InspectorHome),
        KeyCode::End => Some(Action::InspectorEnd),
        KeyCode::Delete => Some(Action::InspectorDelete),
        KeyCode::Enter => Some(Action::InspectorEnter),
        KeyCode::Esc => Some(Action::LeaveMode),
        KeyCode::Tab => Some(Action::ToggleInspector),
        KeyCode::Char(':') => Some(Action::EnterCommand),
        KeyCode::Backspace => Some(Action::InspectorBackspace),
        KeyCode::Char(c) => Some(Action::InspectorChar(c)),
        _ => None,
    }
}
