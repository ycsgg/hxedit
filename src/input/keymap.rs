use crossterm::event::{KeyEvent, KeyEventKind};

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
        Mode::Inspector => inspector::map(key),
    }
}
