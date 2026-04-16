use crossterm::event::{KeyCode, KeyEvent};

use crate::action::Action;
use crate::input::keymap::force_quit_action;

pub fn map(key: KeyEvent) -> Option<Action> {
    if let Some(action) = force_quit_action(&key) {
        return Some(action);
    }

    match key.code {
        // Navigation
        KeyCode::Up | KeyCode::Char('k') => Some(Action::InspectorUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::InspectorDown),
        KeyCode::Left => Some(Action::InspectorLeft),
        KeyCode::Right => Some(Action::InspectorRight),
        KeyCode::Home => Some(Action::InspectorHome),
        KeyCode::End => Some(Action::InspectorEnd),
        KeyCode::Delete => Some(Action::InspectorDelete),

        // Space: toggle collapse/expand on the selected header. When a field is
        // being edited the space still reaches the buffer via Char(' ').
        KeyCode::Char(' ') => Some(Action::InspectorToggleCollapse),

        // Begin / submit edit, or toggle collapse when a header is selected
        // (the branching happens in the event handler so this map stays simple).
        KeyCode::Enter => Some(Action::InspectorEnter),

        // Leave current inspector sub-mode (edit -> inspector, inspector -> normal)
        KeyCode::Esc => Some(Action::LeaveMode),

        // Toggle inspector panel / focus
        KeyCode::Tab => Some(Action::ToggleInspector),

        // Enter command mode from inspector
        KeyCode::Char(':') => Some(Action::EnterCommand),

        // Backspace while editing
        KeyCode::Backspace => Some(Action::InspectorBackspace),

        // Other chars become input when editing
        KeyCode::Char(c) => Some(Action::InspectorChar(c)),

        _ => None,
    }
}
