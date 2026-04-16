use crate::app::{App, StatusLevel};
use crate::mode::Mode;

impl App {
    pub(crate) fn toggle_visual(&mut self) {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Normal => {
                self.selection_anchor = Some(self.cursor);
                self.mode = Mode::Visual;
            }
            Mode::EditHex { .. }
            | Mode::InsertHex { .. }
            | Mode::Command
            | Mode::Inspector
            | Mode::InspectorEdit => {}
        }
    }

    pub(crate) fn clear_error_if_command_done(&mut self) {
        if !matches!(self.mode, Mode::Command) && self.status_level == StatusLevel::Error {
            self.clear_status();
        }
    }

    /// Leave the current mode (Esc handler).
    pub(crate) fn leave_mode(&mut self) {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Command => {
                let return_mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
                self.mode = self.normalize_mode(return_mode);
            }
            Mode::InsertHex { .. } => {
                self.commit_pending_insert();
                self.mode = Mode::Normal;
            }
            Mode::Inspector => {
                if let Some(inspector) = self.inspector.as_mut() {
                    inspector.editing = None;
                }
                self.mode = Mode::Normal;
            }
            Mode::InspectorEdit => {
                if let Some(inspector) = self.inspector.as_mut() {
                    inspector.editing = None;
                }
                self.mode = Mode::Inspector;
            }
            Mode::EditHex { .. } | Mode::Normal => {
                self.mode = Mode::Normal;
            }
        }
    }

    pub(crate) fn normalize_mode(&self, mode: Mode) -> Mode {
        match mode {
            Mode::Inspector | Mode::InspectorEdit
                if !self.show_inspector || !self.inspector_panel_visible() =>
            {
                Mode::Normal
            }
            Mode::InspectorEdit
                if self
                    .inspector
                    .as_ref()
                    .and_then(|inspector| inspector.editing.as_ref())
                    .is_none() =>
            {
                Mode::Inspector
            }
            other => other,
        }
    }

    pub(crate) fn selection_range(&self) -> Option<(u64, u64)> {
        let anchor = self.selection_anchor?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }
}
