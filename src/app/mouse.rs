use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::app::App;
use crate::error::HxResult;
use crate::mode::{Mode, NibblePhase};

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse_event: MouseEvent) -> HxResult<()> {
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
