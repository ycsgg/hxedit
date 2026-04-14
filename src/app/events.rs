use anyhow::Result;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::action::Action;
use crate::app::App;
use crate::error::HxError;
use crate::mode::{Mode, NibblePhase};

impl App {
    pub(crate) fn handle_action(&mut self, action: Action) -> Result<()> {
        let result: crate::error::HxResult<()> = match action.clone() {
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
        };

        match result {
            Ok(()) => {
                self.ensure_cursor_visible();
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
                self.scroll_viewport(-3);
                Ok(())
            }
            MouseEventKind::ScrollDown => {
                self.scroll_viewport(3);
                Ok(())
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(columns) = self.last_columns else {
                    return Ok(());
                };

                if let Some(hit) = crate::input::mouse::hit_test(
                    columns,
                    mouse_event.column,
                    mouse_event.row,
                    self.viewport_top,
                    self.config.bytes_per_line,
                    self.document.len(),
                ) {
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
                        Mode::Normal => {}
                    }
                    self.ensure_cursor_visible();
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
