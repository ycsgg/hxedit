use crate::action::Action;
use crate::app::App;
use crate::error::{HxError, HxResult};
use crate::format::parse::InspectorRow;
use crate::mode::Mode;
use crate::mode::NibblePhase;

impl App {
    pub(crate) fn handle_action(&mut self, action: Action) -> HxResult<()> {
        let result = self.dispatch_action(action);
        self.finish_action(action, result)
    }
}

impl App {
    fn dispatch_action(&mut self, action: Action) -> HxResult<()> {
        if let Some(result) = self.handle_navigation_action(action) {
            return result;
        }
        if let Some(result) = self.handle_command_action(action) {
            return result;
        }
        if let Some(result) = self.handle_inspector_action(action) {
            return result;
        }
        self.handle_editor_action(action)
    }

    fn handle_navigation_action(&mut self, action: Action) -> Option<HxResult<()>> {
        let result = match action {
            Action::MoveLeft => self.move_horizontal(-1),
            Action::MoveRight => self.move_horizontal(1),
            Action::MoveUp => self.move_vertical(-1),
            Action::MoveDown => self.move_vertical(1),
            Action::PageUp => self.move_vertical(-(self.view_rows as i64)),
            Action::PageDown => self.move_vertical(self.view_rows as i64),
            Action::RowStart => self.move_row_edge(false),
            Action::RowEnd => self.move_row_edge(true),
            _ => return None,
        };
        Some(result)
    }

    fn handle_editor_action(&mut self, action: Action) -> HxResult<()> {
        match action {
            Action::ToggleVisual => self.toggle_visual(),
            Action::EnterInsert => self.enter_hex_mode(true),
            Action::EnterReplace => self.enter_hex_mode(false),
            Action::EnterCommand => self.enter_command_mode(),
            Action::LeaveMode => self.leave_mode(),
            Action::DeleteByte => self.delete_at_cursor_or_selection(),
            Action::SearchNext => self.repeat_search(crate::app::SearchDirection::Forward),
            Action::SearchPrev => self.repeat_search(crate::app::SearchDirection::Backward),
            Action::Undo(steps) => self.handle_undo_action(steps),
            Action::EditHex(value) => self.handle_edit_hex_action(value),
            Action::EditBackspace => self.edit_backspace(),
            Action::ForceQuit => {
                self.should_quit = true;
                Ok(())
            }
            Action::ToggleInspector => {
                self.toggle_inspector_mode();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn handle_command_action(&mut self, action: Action) -> Option<HxResult<()>> {
        let result = match action {
            Action::CommandChar(c) => self.insert_command_char(c),
            Action::CommandLeft => self.move_command_cursor_left(),
            Action::CommandRight => self.move_command_cursor_right(),
            Action::CommandHome => self.move_command_cursor_home(),
            Action::CommandEnd => self.move_command_cursor_end(),
            Action::CommandDelete => self.delete_command_char(),
            Action::CommandBackspace => self.backspace_command_char(),
            Action::CommandSubmit => self.submit_command(),
            Action::CommandCancel => self.cancel_command_input(),
            _ => return None,
        };
        Some(result)
    }

    fn handle_inspector_action(&mut self, action: Action) -> Option<HxResult<()>> {
        let result = match action {
            Action::InspectorUp => self.move_inspector_selection(true),
            Action::InspectorDown => self.move_inspector_selection(false),
            Action::InspectorEnter => self.handle_inspector_enter(),
            Action::InspectorChar(c) => self.insert_inspector_char(c),
            Action::InspectorBackspace => self.backspace_inspector_char(),
            Action::InspectorLeft => self.move_inspector_cursor(true),
            Action::InspectorRight => self.move_inspector_cursor(false),
            Action::InspectorHome => self.set_inspector_cursor(true),
            Action::InspectorEnd => self.set_inspector_cursor(false),
            Action::InspectorDelete => self.delete_inspector_char(),
            _ => return None,
        };
        Some(result)
    }

    fn finish_action(&mut self, action: Action, result: HxResult<()>) -> HxResult<()> {
        match result {
            Ok(()) => {
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
                if !is_command_edit_action(action) {
                    self.clear_error_if_command_done();
                }
                Ok(())
            }
            Err(err) => {
                self.status_message = err.to_string();
                Ok(())
            }
        }
    }

    fn enter_hex_mode(&mut self, insert: bool) -> HxResult<()> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        self.mode = if insert {
            Mode::InsertHex { pending: None }
        } else {
            Mode::EditHex {
                phase: NibblePhase::High,
            }
        };
        Ok(())
    }

    fn enter_command_mode(&mut self) -> HxResult<()> {
        let return_mode = if matches!(self.mode, Mode::InsertHex { .. }) {
            self.commit_pending_insert()?;
            Mode::Normal
        } else {
            self.mode
        };
        self.command_return_mode = Some(return_mode);
        self.mode = Mode::Command;
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        Ok(())
    }

    fn handle_undo_action(&mut self, steps: usize) -> HxResult<()> {
        if self.undo_pending_insert()? {
            if steps > 1 {
                self.undo(steps - 1, true)
            } else {
                Ok(())
            }
        } else {
            self.undo(steps, true)
        }
    }

    fn handle_edit_hex_action(&mut self, value: u8) -> HxResult<()> {
        match self.mode {
            Mode::InsertHex { .. } => self.insert_nibble(value),
            _ => self.edit_nibble(value),
        }
    }

    fn move_inspector_selection(&mut self, upward: bool) -> HxResult<()> {
        let Some(inspector) = self.inspector.as_mut() else {
            return Ok(());
        };
        if inspector.editing.is_some() {
            return Ok(());
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

            if matches!(inspector.rows.get(target), Some(InspectorRow::Field { .. })) {
                inspector.selected_row = target;
                break;
            }
        }

        self.sync_cursor_to_inspector();
        Ok(())
    }

    fn handle_inspector_enter(&mut self) -> HxResult<()> {
        let Some(inspector) = self.inspector.as_mut() else {
            return Ok(());
        };
        if inspector.editing.is_some() {
            return self.submit_inspector_edit();
        }

        if let Some(InspectorRow::Field {
            editable: true,
            display,
            ..
        }) = inspector.rows.get(inspector.selected_row)
        {
            inspector.editing = Some(crate::app::InspectorEdit {
                row_index: inspector.selected_row,
                buffer: display.clone(),
                cursor_pos: display.len(),
            });
            self.mode = Mode::InspectorEdit;
        }
        Ok(())
    }

    fn insert_inspector_char(&mut self, c: char) -> HxResult<()> {
        if let Some(inspector) = self.inspector.as_mut() {
            if let Some(edit) = inspector.editing.as_mut() {
                edit.buffer.insert(edit.cursor_pos, c);
                edit.cursor_pos += c.len_utf8();
            } else if c == 't' {
                self.toggle_inspector_mode();
            }
        }
        Ok(())
    }

    fn backspace_inspector_char(&mut self) -> HxResult<()> {
        if let Some(edit) = self.inspector_edit_mut() {
            if edit.cursor_pos > 0 {
                edit.cursor_pos = prev_char_boundary(&edit.buffer, edit.cursor_pos);
                edit.buffer.remove(edit.cursor_pos);
            }
        }
        Ok(())
    }

    fn move_inspector_cursor(&mut self, left: bool) -> HxResult<()> {
        if let Some(edit) = self.inspector_edit_mut() {
            edit.cursor_pos = if left {
                prev_char_boundary(&edit.buffer, edit.cursor_pos)
            } else {
                next_char_boundary(&edit.buffer, edit.cursor_pos)
            };
        }
        Ok(())
    }

    fn set_inspector_cursor(&mut self, home: bool) -> HxResult<()> {
        if let Some(edit) = self.inspector_edit_mut() {
            edit.cursor_pos = if home { 0 } else { edit.buffer.len() };
        }
        Ok(())
    }

    fn delete_inspector_char(&mut self) -> HxResult<()> {
        if let Some(edit) = self.inspector_edit_mut() {
            if edit.cursor_pos < edit.buffer.len() {
                edit.buffer.remove(edit.cursor_pos);
            }
        }
        Ok(())
    }

    fn inspector_edit_mut(&mut self) -> Option<&mut crate::app::InspectorEdit> {
        self.inspector.as_mut()?.editing.as_mut()
    }
}

fn prev_char_boundary(text: &str, cursor_pos: usize) -> usize {
    text[..cursor_pos.min(text.len())]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor_pos: usize) -> usize {
    if cursor_pos >= text.len() {
        return text.len();
    }
    let start = cursor_pos.min(text.len());
    text[start..]
        .char_indices()
        .nth(1)
        .map(|(idx, _)| start + idx)
        .unwrap_or(text.len())
}

fn is_command_edit_action(action: Action) -> bool {
    matches!(
        action,
        Action::CommandChar(_)
            | Action::CommandLeft
            | Action::CommandRight
            | Action::CommandHome
            | Action::CommandEnd
            | Action::CommandDelete
            | Action::CommandBackspace
    )
}
#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::action::Action;
    use crate::cli::Cli;
    use crate::format::parse::{FieldValue, StructValue};
    use crate::format::types::{FieldDef, FieldType};

    fn app_with_len(len: usize) -> App {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, vec![0_u8; len]).unwrap();
        let cli = Cli {
            file,
            bytes_per_line: 16,
            page_size: 4096,
            cache_pages: 8,
            profile: false,
            readonly: false,
            no_color: true,
            offset: None,
            inspector: false,
        };
        let mut app = App::from_cli(cli).unwrap();
        app.view_rows = 4;
        app
    }

    fn app_with_inspector_field() -> App {
        let mut app = app_with_len(4);
        let field = FieldDef {
            name: "entry".to_owned(),
            offset: 0,
            field_type: FieldType::U8,
            description: String::new(),
            editable: true,
        };
        let structs = vec![StructValue {
            name: "Header".to_owned(),
            base_offset: 0,
            fields: vec![FieldValue {
                def: field,
                abs_offset: 0,
                raw_bytes: vec![0],
                display: "0x00".to_owned(),
                size: 1,
            }],
            children: Vec::new(),
        }];
        let rows = crate::format::parse::flatten(&structs);
        app.show_inspector = true;
        app.inspector = Some(crate::app::InspectorState {
            format_name: "TEST".to_owned(),
            structs,
            rows,
            scroll_offset: 0,
            selected_row: 1,
            editing: None,
        });
        app.mode = Mode::Inspector;
        app
    }

    #[test]
    fn command_cursor_can_move_and_insert_in_middle() {
        let mut app = app_with_len(4);
        app.handle_action(Action::EnterCommand).unwrap();
        app.handle_action(Action::CommandChar('a')).unwrap();
        app.handle_action(Action::CommandChar('b')).unwrap();
        app.handle_action(Action::CommandChar('c')).unwrap();
        app.handle_action(Action::CommandLeft).unwrap();
        app.handle_action(Action::CommandLeft).unwrap();
        app.handle_action(Action::CommandChar('X')).unwrap();

        assert_eq!(app.command_buffer, "aXbc");
        assert_eq!(app.command_cursor_pos, 2);
    }

    #[test]
    fn command_backspace_respects_cursor_position() {
        let mut app = app_with_len(4);
        app.handle_action(Action::EnterCommand).unwrap();
        app.handle_action(Action::CommandChar('a')).unwrap();
        app.handle_action(Action::CommandChar('b')).unwrap();
        app.handle_action(Action::CommandChar('c')).unwrap();
        app.handle_action(Action::CommandLeft).unwrap();
        app.handle_action(Action::CommandBackspace).unwrap();

        assert_eq!(app.command_buffer, "ac");
        assert_eq!(app.command_cursor_pos, 1);
    }

    #[test]
    fn command_delete_home_and_end_respect_cursor_position() {
        let mut app = app_with_len(4);
        app.handle_action(Action::EnterCommand).unwrap();
        app.handle_action(Action::CommandChar('a')).unwrap();
        app.handle_action(Action::CommandChar('b')).unwrap();
        app.handle_action(Action::CommandChar('c')).unwrap();
        app.handle_action(Action::CommandChar('d')).unwrap();

        app.handle_action(Action::CommandHome).unwrap();
        assert_eq!(app.command_cursor_pos, 0);

        app.handle_action(Action::CommandRight).unwrap();
        app.handle_action(Action::CommandRight).unwrap();
        app.handle_action(Action::CommandDelete).unwrap();

        assert_eq!(app.command_buffer, "abd");
        assert_eq!(app.command_cursor_pos, 2);

        app.handle_action(Action::CommandEnd).unwrap();
        assert_eq!(app.command_cursor_pos, app.command_buffer.len());
    }

    #[test]
    fn inspector_escape_returns_to_inspector_mode() {
        let mut app = app_with_inspector_field();

        app.handle_action(Action::InspectorEnter).unwrap();
        assert_eq!(app.mode, Mode::InspectorEdit);
        assert!(app
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.editing.as_ref())
            .is_some());

        app.handle_action(Action::LeaveMode).unwrap();

        assert_eq!(app.mode, Mode::Inspector);
        assert!(app
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.editing.as_ref())
            .is_none());
    }
}
