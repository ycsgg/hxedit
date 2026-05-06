use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::input::keymap::force_quit_action;

pub fn map(key: KeyEvent) -> Option<Action> {
    if let Some(action) = force_quit_action(&key) {
        return Some(action);
    }

    match key.code {
        // Navigation
        KeyCode::Up | KeyCode::Char('k') => Some(Action::SidePanelUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::SidePanelDown),
        KeyCode::Left => Some(Action::SidePanelLeft),
        KeyCode::Right => Some(Action::SidePanelRight),
        KeyCode::Home => Some(Action::SidePanelHome),
        KeyCode::End => Some(Action::SidePanelEnd),
        KeyCode::Delete => Some(Action::SidePanelDelete),

        // Space: toggle collapse/expand on the selected header. When a field is
        // being edited the space still reaches the buffer via Char(' ').
        KeyCode::Char(' ') => Some(Action::SidePanelToggleCollapse),

        // Begin / submit edit, or toggle collapse when a header is selected
        // (the branching happens in the event handler so this map stays simple).
        KeyCode::Enter => Some(Action::SidePanelEnter),

        // Leave current side-panel sub-mode (edit -> panel, panel -> normal).
        KeyCode::Esc => Some(Action::LeaveMode),

        // Toggle side-panel visibility / focus.
        KeyCode::Tab => Some(Action::ToggleSidePanel),

        // Enter command mode while the side panel owns focus.
        KeyCode::Char(':') => Some(Action::EnterCommand),

        // Search should keep working even while a side panel owns focus.
        KeyCode::Char('n') => Some(Action::SearchNext),
        KeyCode::Char('p') => Some(Action::SearchPrev),

        // Backspace while editing.
        KeyCode::Backspace => Some(Action::SidePanelBackspace),

        // Other chars become input when editing.
        KeyCode::Char(c) => Some(Action::SidePanelChar(c)),

        _ => None,
    }
}
