use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::action::Action;
use crate::input::{command, edit, inspector, normal, visual};
use crate::mode::Mode;

pub fn map_key(mode: Mode, key: KeyEvent) -> Option<Action> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match mode {
        Mode::Normal => normal::map(key),
        Mode::EditHex { .. } | Mode::InsertHex { .. } => edit::map(key),
        Mode::Visual => visual::map(key),
        Mode::Command => command::map(key),
        Mode::Inspector | Mode::InspectorEdit => inspector::map(key),
    }
}

pub(crate) fn movement_action(code: &KeyCode) -> Option<Action> {
    match code {
        KeyCode::Left | KeyCode::Char('h') => Some(Action::MoveLeft),
        KeyCode::Right | KeyCode::Char('l') => Some(Action::MoveRight),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home => Some(Action::RowStart),
        KeyCode::End => Some(Action::RowEnd),
        _ => None,
    }
}

pub(crate) fn force_quit_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::ForceQuit)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{force_quit_action, movement_action};
    use crate::action::Action;

    #[test]
    fn movement_action_covers_shared_navigation_keys() {
        assert_eq!(movement_action(&KeyCode::Left), Some(Action::MoveLeft));
        assert_eq!(
            movement_action(&KeyCode::Char('l')),
            Some(Action::MoveRight)
        );
        assert_eq!(movement_action(&KeyCode::PageDown), Some(Action::PageDown));
        assert_eq!(movement_action(&KeyCode::End), Some(Action::RowEnd));
        assert_eq!(movement_action(&KeyCode::Char('x')), None);
    }

    #[test]
    fn force_quit_action_requires_ctrl_c() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let plain_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);

        assert_eq!(force_quit_action(&ctrl_c), Some(Action::ForceQuit));
        assert_eq!(force_quit_action(&plain_c), None);
    }
}
