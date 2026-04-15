use anyhow::Result;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::action::Action;
use crate::app::App;
use crate::error::HxError;
use crate::format::parse::InspectorRow;
use crate::mode::{Mode, NibblePhase};

impl App {
    pub(crate) fn handle_action(&mut self, action: Action) -> Result<()> {
        let result: crate::error::HxResult<()> = match action {
            Action::MoveLeft => self.move_horizontal(-1),
            Action::MoveRight => self.move_horizontal(1),
            Action::MoveUp => self.move_vertical(-1),
            Action::MoveDown => self.move_vertical(1),
            Action::PageUp => self.move_vertical(-(self.view_rows as i64)),
            Action::PageDown => self.move_vertical(self.view_rows as i64),
            Action::RowStart => self.move_row_edge(false),
            Action::RowEnd => self.move_row_edge(true),
            Action::ToggleVisual => self.toggle_visual(),
            Action::EnterInsert => {
                if self.document.is_readonly() {
                    Err(HxError::ReadOnly)
                } else {
                    self.mode = Mode::InsertHex { pending: None };
                    Ok(())
                }
            }
            Action::EnterReplace => {
                if self.document.is_readonly() {
                    Err(HxError::ReadOnly)
                } else {
                    self.mode = Mode::EditHex {
                        phase: NibblePhase::High,
                    };
                    Ok(())
                }
            }
            Action::EnterCommand => {
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
            Action::LeaveMode => self.leave_mode(),
            Action::DeleteByte => self.delete_at_cursor_or_selection(),
            Action::SearchNext => self.repeat_search(crate::app::SearchDirection::Forward),
            Action::SearchPrev => self.repeat_search(crate::app::SearchDirection::Backward),
            Action::Undo(steps) => {
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
            Action::EditHex(value) => match self.mode {
                Mode::InsertHex { .. } => self.insert_nibble(value),
                _ => self.edit_nibble(value),
            },
            Action::EditBackspace => self.edit_backspace(),
            Action::CommandChar(c) => {
                let pos = self.command_cursor_pos.min(self.command_buffer.len());
                self.command_buffer.insert(pos, c);
                self.command_cursor_pos = pos + c.len_utf8();
                Ok(())
            }
            Action::CommandLeft => {
                self.command_cursor_pos =
                    prev_char_boundary(&self.command_buffer, self.command_cursor_pos);
                Ok(())
            }
            Action::CommandRight => {
                self.command_cursor_pos =
                    next_char_boundary(&self.command_buffer, self.command_cursor_pos);
                Ok(())
            }
            Action::CommandHome => {
                self.command_cursor_pos = 0;
                Ok(())
            }
            Action::CommandEnd => {
                self.command_cursor_pos = self.command_buffer.len();
                Ok(())
            }
            Action::CommandDelete => {
                if self.command_cursor_pos < self.command_buffer.len() {
                    let next = next_char_boundary(&self.command_buffer, self.command_cursor_pos);
                    self.command_buffer
                        .replace_range(self.command_cursor_pos..next, "");
                }
                Ok(())
            }
            Action::CommandBackspace => {
                if self.command_cursor_pos > 0 {
                    let prev = prev_char_boundary(&self.command_buffer, self.command_cursor_pos);
                    self.command_buffer
                        .replace_range(prev..self.command_cursor_pos, "");
                    self.command_cursor_pos = prev;
                }
                Ok(())
            }
            Action::CommandSubmit => self.submit_command(),
            Action::CommandCancel => {
                self.command_buffer.clear();
                self.command_cursor_pos = 0;
                let return_mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
                self.mode = self.normalize_mode(return_mode);
                Ok(())
            }
            Action::ForceQuit => {
                self.should_quit = true;
                Ok(())
            }
            Action::ToggleInspector => {
                self.toggle_inspector_mode();
                Ok(())
            }
            Action::InspectorUp => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if inspector.editing.is_some() {
                        return Ok(());
                    }
                    let mut target = inspector.selected_row;
                    loop {
                        if target == 0 {
                            break;
                        }
                        target -= 1;
                        if matches!(inspector.rows.get(target), Some(InspectorRow::Field { .. })) {
                            inspector.selected_row = target;
                            break;
                        }
                    }
                    self.sync_cursor_to_inspector();
                }
                Ok(())
            }
            Action::InspectorDown => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if inspector.editing.is_some() {
                        return Ok(());
                    }
                    let mut target = inspector.selected_row;
                    loop {
                        target += 1;
                        if target >= inspector.rows.len() {
                            break;
                        }
                        if matches!(inspector.rows.get(target), Some(InspectorRow::Field { .. })) {
                            inspector.selected_row = target;
                            break;
                        }
                    }
                    self.sync_cursor_to_inspector();
                }
                Ok(())
            }
            Action::InspectorEnter => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if inspector.editing.is_some() {
                        self.submit_inspector_edit()?;
                    } else if let Some(InspectorRow::Field {
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
                }
                Ok(())
            }
            Action::InspectorChar(c) => {
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
            Action::InspectorBackspace => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if let Some(edit) = inspector.editing.as_mut() {
                        if edit.cursor_pos > 0 {
                            edit.cursor_pos = prev_char_boundary(&edit.buffer, edit.cursor_pos);
                            edit.buffer.remove(edit.cursor_pos);
                        }
                    }
                }
                Ok(())
            }
            Action::InspectorLeft => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if let Some(edit) = inspector.editing.as_mut() {
                        edit.cursor_pos = prev_char_boundary(&edit.buffer, edit.cursor_pos);
                    }
                }
                Ok(())
            }
            Action::InspectorRight => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if let Some(edit) = inspector.editing.as_mut() {
                        edit.cursor_pos = next_char_boundary(&edit.buffer, edit.cursor_pos);
                    }
                }
                Ok(())
            }
            Action::InspectorHome => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if let Some(edit) = inspector.editing.as_mut() {
                        edit.cursor_pos = 0;
                    }
                }
                Ok(())
            }
            Action::InspectorEnd => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if let Some(edit) = inspector.editing.as_mut() {
                        edit.cursor_pos = edit.buffer.len();
                    }
                }
                Ok(())
            }
            Action::InspectorDelete => {
                if let Some(inspector) = self.inspector.as_mut() {
                    if let Some(edit) = inspector.editing.as_mut() {
                        if edit.cursor_pos < edit.buffer.len() {
                            edit.buffer.remove(edit.cursor_pos);
                        }
                    }
                }
                Ok(())
            }
        };

        match result {
            Ok(()) => {
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
                if !matches!(
                    action,
                    Action::CommandChar(_)
                        | Action::CommandLeft
                        | Action::CommandRight
                        | Action::CommandHome
                        | Action::CommandEnd
                        | Action::CommandDelete
                        | Action::CommandBackspace
                ) {
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

    pub(crate) fn handle_mouse(&mut self, mouse_event: MouseEvent) -> Result<()> {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                let over_inspector = self
                    .last_columns
                    .and_then(|columns| columns.inspector)
                    .is_some_and(|area| {
                        crate::app::helpers::contains(area, mouse_event.column, mouse_event.row)
                    });
                if over_inspector {
                    self.scroll_inspector(-3);
                } else {
                    self.scroll_viewport(-3);
                    self.sync_inspector_to_cursor();
                }
                Ok(())
            }
            MouseEventKind::ScrollDown => {
                let over_inspector = self
                    .last_columns
                    .and_then(|columns| columns.inspector)
                    .is_some_and(|area| {
                        crate::app::helpers::contains(area, mouse_event.column, mouse_event.row)
                    });
                if over_inspector {
                    self.scroll_inspector(3);
                } else {
                    self.scroll_viewport(3);
                    self.sync_inspector_to_cursor();
                }
                Ok(())
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(columns) = self.last_columns else {
                    return Ok(());
                };

                if columns.inspector.is_some_and(|area| {
                    crate::app::helpers::contains(area, mouse_event.column, mouse_event.row)
                }) {
                    if !self.mode.is_inspector() {
                        self.leave_mode()?;
                    }
                    self.mode = Mode::Inspector;
                }

                if let Some(hit) = crate::input::mouse::hit_test(
                    columns,
                    mouse_event.column,
                    mouse_event.row,
                    self.viewport_top,
                    self.config.bytes_per_line,
                    self.document.len(),
                ) {
                    if let Some(visible_row) = hit.inspector_row {
                        if self.show_inspector {
                            self.mode = Mode::Inspector;
                        }
                        if self.show_inspector && self.inspector.is_some() {
                            let width = columns.inspector.map(|area| area.width).unwrap_or(32);
                            let actual_visual_row = self
                                .inspector
                                .as_ref()
                                .map(|inspector| inspector.scroll_offset + visible_row)
                                .unwrap_or(visible_row);
                            if let Some(row_index) = self
                                .inspector_rendered_lines(width)
                                .get(actual_visual_row)
                                .map(|line| line.row_index)
                            {
                                self.set_inspector_selected_row(row_index);
                                self.sync_cursor_to_inspector();
                            }
                        }
                        return Ok(());
                    }

                    if self.mode.is_inspector() {
                        self.leave_mode()?;
                    }
                    if matches!(self.mode, Mode::InsertHex { .. }) {
                        self.commit_pending_insert()?;
                    }
                    self.mouse_selection_anchor = Some(hit.offset);
                    self.cursor = hit.offset;
                    match self.mode {
                        Mode::EditHex { .. } => {
                            self.mode = Mode::EditHex {
                                phase: hit.phase.unwrap_or(NibblePhase::High),
                            };
                        }
                        Mode::InsertHex { .. } => {
                            self.mode = Mode::Normal;
                        }
                        Mode::Command => {
                            self.command_buffer.clear();
                            self.command_cursor_pos = 0;
                            let return_mode =
                                self.command_return_mode.take().unwrap_or(Mode::Normal);
                            self.mode = self.normalize_mode(return_mode);
                        }
                        Mode::Visual => {
                            self.selection_anchor = None;
                            self.mode = Mode::Normal;
                        }
                        Mode::Normal | Mode::Inspector | Mode::InspectorEdit => {}
                    }
                    self.ensure_cursor_visible();
                    self.sync_inspector_to_cursor();
                    return Ok(());
                }

                if matches!(self.mode, Mode::Command)
                    && self.last_command_area.is_some_and(|rect| {
                        crate::app::helpers::contains(rect, mouse_event.column, mouse_event.row)
                    })
                {
                    return Ok(());
                }

                Ok(())
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(columns) = self.last_columns else {
                    return Ok(());
                };
                let Some(hit) = crate::input::mouse::hit_test(
                    columns,
                    mouse_event.column,
                    mouse_event.row,
                    self.viewport_top,
                    self.config.bytes_per_line,
                    self.document.len(),
                ) else {
                    return Ok(());
                };

                let anchor = self.mouse_selection_anchor.unwrap_or(hit.offset);
                self.selection_anchor = Some(anchor);
                self.cursor = hit.offset;
                self.mode = Mode::Visual;
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
                Ok(())
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_selection_anchor = None;
                Ok(())
            }
            _ => Ok(()),
        }
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
