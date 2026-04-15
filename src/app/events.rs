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
                self.command_buffer.push(c);
                Ok(())
            }
            Action::CommandBackspace => {
                self.command_buffer.pop();
                Ok(())
            }
            Action::CommandSubmit => self.submit_command(),
            Action::CommandCancel => {
                self.command_buffer.clear();
                self.mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
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
                if !matches!(action, Action::CommandChar(_) | Action::CommandBackspace) {
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
                    if !matches!(self.mode, Mode::Inspector) {
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

                    if matches!(self.mode, Mode::Inspector) {
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
                            self.mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
                        }
                        Mode::Visual => {
                            self.selection_anchor = None;
                            self.mode = Mode::Normal;
                        }
                        Mode::Normal | Mode::Inspector => {}
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
