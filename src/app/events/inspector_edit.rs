use super::*;

impl App {
    pub(super) fn move_inspector_selection(&mut self, upward: bool) {
        if self.active_side_panel != SidePanelKind::Inspector {
            return;
        }
        let Some(inspector) = self.inspector_mut() else {
            return;
        };
        if inspector.editing.is_some() {
            return;
        }

        let mut target = inspector.selected_row;
        loop {
            if upward {
                if target == 0 {
                    break;
                }
                target -= 1;
            } else {
                target += 1;
                if target >= inspector.rows.len() {
                    break;
                }
            }

            if inspector
                .rows
                .get(target)
                .is_some_and(crate::app::inspector_state::is_selectable)
            {
                inspector.selected_row = target;
                break;
            }
        }

        self.sync_cursor_to_inspector();
    }

    pub(super) fn handle_inspector_enter(&mut self) -> HxResult<()> {
        let Some(inspector) = self.inspector_mut() else {
            return Ok(());
        };
        if inspector.editing.is_some() {
            return self.submit_inspector_edit();
        }

        let Some(row) = inspector.rows.get(inspector.selected_row).cloned() else {
            return Ok(());
        };

        match row {
            InspectorRow::Field {
                editable: true,
                display,
                ..
            } => {
                inspector.editing = Some(crate::app::InspectorEdit {
                    row_index: inspector.selected_row,
                    buffer: display.clone(),
                    cursor_pos: display.len(),
                });
                self.mode = Mode::InspectorEdit;
                if let Some(warning) = self.inspector_edit_warning() {
                    self.set_warning_status(warning);
                }
            }
            InspectorRow::Field {
                editable: false,
                name,
                ..
            } => {
                let format_name = inspector.format_name.clone();
                self.set_info_status(self.inspector_read_only_message(&format_name, &name));
            }
            InspectorRow::Header {
                has_children: true, ..
            } => {
                // Header Enter = toggle — consistent with ImHex / file tree UIs.
                self.toggle_inspector_collapse();
            }
            InspectorRow::Header { .. } => {}
        }
        Ok(())
    }

    pub(super) fn insert_inspector_char(&mut self, c: char) {
        if let Some(inspector) = self.inspector_mut() {
            if let Some(edit) = inspector.editing.as_mut() {
                insert_char_at_cursor(&mut edit.buffer, &mut edit.cursor_pos, c);
            } else if c == 't' {
                self.toggle_side_panel();
            }
        }
    }

    pub(super) fn backspace_inspector_char(&mut self) {
        if let Some(edit) = self.inspector_edit_mut() {
            backspace_char_before_cursor(&mut edit.buffer, &mut edit.cursor_pos);
        }
    }

    pub(super) fn move_inspector_cursor(&mut self, left: bool) {
        if let Some(edit) = self.inspector_edit_mut() {
            if left {
                move_cursor_left(&edit.buffer, &mut edit.cursor_pos);
            } else {
                move_cursor_right(&edit.buffer, &mut edit.cursor_pos);
            }
        }
    }

    pub(super) fn set_inspector_cursor(&mut self, home: bool) {
        if let Some(edit) = self.inspector_edit_mut() {
            if home {
                move_cursor_home(&mut edit.cursor_pos);
            } else {
                move_cursor_end(&edit.buffer, &mut edit.cursor_pos);
            }
        }
    }

    pub(super) fn delete_inspector_char(&mut self) {
        if let Some(edit) = self.inspector_edit_mut() {
            delete_char_at_cursor(&mut edit.buffer, edit.cursor_pos);
        }
    }

    pub(super) fn inspector_edit_mut(&mut self) -> Option<&mut crate::app::InspectorEdit> {
        self.inspector_mut()?.editing.as_mut()
    }
}
