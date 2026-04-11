use crossterm::event::{KeyEvent, KeyEventKind};

use crate::action::Action;
use crate::input::{command, edit, normal};
use crate::mode::Mode;

pub fn map_key(mode: Mode, key: KeyEvent) -> Option<Action> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match mode {
        Mode::Normal => normal::map(key),
        Mode::EditHex { .. } => edit::map(key),
        Mode::Command => command::map(key),
    }
}
