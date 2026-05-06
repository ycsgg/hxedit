use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::input::keymap::force_quit_action;

pub fn map(key: KeyEvent) -> Option<Action> {
    if let Some(action) = force_quit_action(&key) {
        return Some(action);
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::SidePanelUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::SidePanelDown),
        KeyCode::Left => Some(Action::SidePanelLeft),
        KeyCode::Right => Some(Action::SidePanelRight),
        KeyCode::Home => Some(Action::SidePanelHome),
        KeyCode::End => Some(Action::SidePanelEnd),
        KeyCode::Delete => Some(Action::SidePanelDelete),
        KeyCode::Enter => Some(Action::SidePanelEnter),
        KeyCode::Esc => Some(Action::LeaveMode),
        KeyCode::Tab => Some(Action::ToggleSidePanel),
        KeyCode::Char(':') => Some(Action::EnterCommand),
        KeyCode::Backspace => Some(Action::SidePanelBackspace),
        KeyCode::Char(c) => Some(Action::SidePanelChar(c)),
        _ => None,
    }
}
