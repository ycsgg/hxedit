use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;

pub fn map(key: KeyEvent) -> Option<Action> {
    match key.code {
        // Navigation
        KeyCode::Up | KeyCode::Char('k') => Some(Action::InspectorUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::InspectorDown),
        KeyCode::Left => Some(Action::InspectorLeft),
        KeyCode::Right => Some(Action::InspectorRight),
        KeyCode::Home => Some(Action::InspectorHome),
        KeyCode::End => Some(Action::InspectorEnd),
        KeyCode::Delete => Some(Action::InspectorDelete),

        // Begin / submit edit
        KeyCode::Enter => Some(Action::InspectorEnter),

        // Exit inspector mode back to normal
        KeyCode::Esc => Some(Action::LeaveMode),

        // Toggle inspector panel / focus
        KeyCode::Tab => Some(Action::ToggleInspector),

        // Enter command mode from inspector
        KeyCode::Char(':') => Some(Action::EnterCommand),

        // Backspace while editing
        KeyCode::Backspace => Some(Action::InspectorBackspace),

        // Ctrl+C force quit
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::ForceQuit)
        }

        // Other chars become input when editing
        KeyCode::Char(c) => Some(Action::InspectorChar(c)),

        _ => None,
    }
}
