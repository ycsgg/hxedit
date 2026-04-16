use crate::action::Action;
use crate::app::text_cursor::{next_char_boundary, prev_char_boundary};
use crate::app::App;
use crate::error::{HxError, HxResult};
use crate::format::parse::InspectorRow;
use crate::mode::Mode;
use crate::mode::NibblePhase;

impl App {
    pub(crate) fn handle_action(&mut self, action: Action) {
        let result = self.dispatch_action(action);
        self.finish_action(action, result);
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
            Action::MoveLeft => {
                self.move_horizontal(-1);
                Ok(())
            }
            Action::MoveRight => {
                self.move_horizontal(1);
                Ok(())
            }
            Action::MoveUp => {
                self.move_vertical(-1);
                Ok(())
            }
            Action::MoveDown => {
                self.move_vertical(1);
                Ok(())
            }
            Action::PageUp => {
                self.move_vertical(-(self.view_rows as i64));
                Ok(())
            }
            Action::PageDown => {
                self.move_vertical(self.view_rows as i64);
                Ok(())
            }
            Action::RowStart => {
                self.move_row_edge(false);
                Ok(())
            }
            Action::RowEnd => {
                self.move_row_edge(true);
                Ok(())
            }
            _ => return None,
        };
        Some(result)
    }

    fn handle_editor_action(&mut self, action: Action) -> HxResult<()> {
        match action {
            Action::ToggleVisual => {
                self.toggle_visual();
                Ok(())
            }
            Action::EnterInsert => self.enter_hex_mode(true),
            Action::EnterReplace => self.enter_hex_mode(false),
            Action::EnterCommand => {
                self.enter_command_mode();
                Ok(())
            }
            Action::LeaveMode => {
                self.leave_mode();
                Ok(())
            }
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
            Action::CommandChar(c) => {
                self.insert_command_char(c);
                Ok(())
            }
            Action::CommandLeft => {
                self.move_command_cursor_left();
                Ok(())
            }
            Action::CommandRight => {
                self.move_command_cursor_right();
                Ok(())
            }
            Action::CommandHome => {
                self.move_command_cursor_home();
                Ok(())
            }
            Action::CommandEnd => {
                self.move_command_cursor_end();
                Ok(())
            }
            Action::CommandDelete => {
                self.delete_command_char();
                Ok(())
            }
            Action::CommandBackspace => {
                self.backspace_command_char();
                Ok(())
            }
            Action::CommandSubmit => self.submit_command(),
            Action::CommandCancel => {
                self.cancel_command_input();
                Ok(())
            }
            _ => return None,
        };
        Some(result)
    }

    fn handle_inspector_action(&mut self, action: Action) -> Option<HxResult<()>> {
        let result = match action {
            Action::InspectorUp => {
                self.move_inspector_selection(true);
                Ok(())
            }
            Action::InspectorDown => {
                self.move_inspector_selection(false);
                Ok(())
            }
            Action::InspectorEnter => self.handle_inspector_enter(),
            Action::InspectorChar(c) => {
                self.insert_inspector_char(c);
                Ok(())
            }
            Action::InspectorBackspace => {
                self.backspace_inspector_char();
                Ok(())
            }
            Action::InspectorLeft => {
                self.move_inspector_cursor(true);
                Ok(())
            }
            Action::InspectorRight => {
                self.move_inspector_cursor(false);
                Ok(())
            }
            Action::InspectorHome => {
                self.set_inspector_cursor(true);
                Ok(())
            }
            Action::InspectorEnd => {
                self.set_inspector_cursor(false);
                Ok(())
            }
            Action::InspectorDelete => {
                self.delete_inspector_char();
                Ok(())
            }
            _ => return None,
        };
        Some(result)
    }

    fn finish_action(&mut self, action: Action, result: HxResult<()>) {
        match result {
            Ok(()) => {
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
                if !is_command_edit_action(action) {
                    self.clear_error_if_command_done();
                }
            }
            Err(err) => {
                self.status_message = err.to_string();
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

    fn enter_command_mode(&mut self) {
        let return_mode = if matches!(self.mode, Mode::InsertHex { .. }) {
            self.commit_pending_insert();
            Mode::Normal
        } else {
            self.mode
        };
        self.command_return_mode = Some(return_mode);
        self.mode = Mode::Command;
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
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

    fn move_inspector_selection(&mut self, upward: bool) {
        let Some(inspector) = self.inspector.as_mut() else {
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

            if matches!(inspector.rows.get(target), Some(InspectorRow::Field { .. })) {
                inspector.selected_row = target;
                break;
            }
        }

        self.sync_cursor_to_inspector();
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

    fn insert_inspector_char(&mut self, c: char) {
        if let Some(inspector) = self.inspector.as_mut() {
            if let Some(edit) = inspector.editing.as_mut() {
                edit.buffer.insert(edit.cursor_pos, c);
                edit.cursor_pos += c.len_utf8();
            } else if c == 't' {
                self.toggle_inspector_mode();
            }
        }
    }

    fn backspace_inspector_char(&mut self) {
        if let Some(edit) = self.inspector_edit_mut() {
            if edit.cursor_pos > 0 {
                edit.cursor_pos = prev_char_boundary(&edit.buffer, edit.cursor_pos);
                edit.buffer.remove(edit.cursor_pos);
            }
        }
    }

    fn move_inspector_cursor(&mut self, left: bool) {
        if let Some(edit) = self.inspector_edit_mut() {
            edit.cursor_pos = if left {
                prev_char_boundary(&edit.buffer, edit.cursor_pos)
            } else {
                next_char_boundary(&edit.buffer, edit.cursor_pos)
            };
        }
    }

    fn set_inspector_cursor(&mut self, home: bool) {
        if let Some(edit) = self.inspector_edit_mut() {
            edit.cursor_pos = if home { 0 } else { edit.buffer.len() };
        }
    }

    fn delete_inspector_char(&mut self) {
        if let Some(edit) = self.inspector_edit_mut() {
            if edit.cursor_pos < edit.buffer.len() {
                edit.buffer.remove(edit.cursor_pos);
            }
        }
    }

    fn inspector_edit_mut(&mut self) -> Option<&mut crate::app::InspectorEdit> {
        self.inspector.as_mut()?.editing.as_mut()
    }
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
        app.handle_action(Action::EnterCommand);
        app.handle_action(Action::CommandChar('a'));
        app.handle_action(Action::CommandChar('b'));
        app.handle_action(Action::CommandChar('c'));
        app.handle_action(Action::CommandLeft);
        app.handle_action(Action::CommandLeft);
        app.handle_action(Action::CommandChar('X'));

        assert_eq!(app.command_buffer, "aXbc");
        assert_eq!(app.command_cursor_pos, 2);
    }

    #[test]
    fn command_backspace_respects_cursor_position() {
        let mut app = app_with_len(4);
        app.handle_action(Action::EnterCommand);
        app.handle_action(Action::CommandChar('a'));
        app.handle_action(Action::CommandChar('b'));
        app.handle_action(Action::CommandChar('c'));
        app.handle_action(Action::CommandLeft);
        app.handle_action(Action::CommandBackspace);

        assert_eq!(app.command_buffer, "ac");
        assert_eq!(app.command_cursor_pos, 1);
    }

    #[test]
    fn command_delete_home_and_end_respect_cursor_position() {
        let mut app = app_with_len(4);
        app.handle_action(Action::EnterCommand);
        app.handle_action(Action::CommandChar('a'));
        app.handle_action(Action::CommandChar('b'));
        app.handle_action(Action::CommandChar('c'));
        app.handle_action(Action::CommandChar('d'));

        app.handle_action(Action::CommandHome);
        assert_eq!(app.command_cursor_pos, 0);

        app.handle_action(Action::CommandRight);
        app.handle_action(Action::CommandRight);
        app.handle_action(Action::CommandDelete);

        assert_eq!(app.command_buffer, "abd");
        assert_eq!(app.command_cursor_pos, 2);

        app.handle_action(Action::CommandEnd);
        assert_eq!(app.command_cursor_pos, app.command_buffer.len());
    }

    #[test]
    fn inspector_escape_returns_to_inspector_mode() {
        let mut app = app_with_inspector_field();

        app.handle_action(Action::InspectorEnter);
        assert_eq!(app.mode, Mode::InspectorEdit);
        assert!(app
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.editing.as_ref())
            .is_some());

        app.handle_action(Action::LeaveMode);

        assert_eq!(app.mode, Mode::Inspector);
        assert!(app
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.editing.as_ref())
            .is_none());
    }
}
