use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::app::App;
use crate::format::parse::InspectorRow;
use crate::mode::{Mode, NibblePhase};
use crate::util::geometry::rect_contains;

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                let over_inspector = self
                    .last_columns
                    .and_then(|columns| columns.inspector)
                    .is_some_and(|area| rect_contains(area, mouse_event.column, mouse_event.row));
                if over_inspector {
                    self.scroll_inspector(-3);
                } else {
                    self.scroll_viewport(-3);
                    self.sync_inspector_to_cursor();
                }
            }
            MouseEventKind::ScrollDown => {
                let over_inspector = self
                    .last_columns
                    .and_then(|columns| columns.inspector)
                    .is_some_and(|area| rect_contains(area, mouse_event.column, mouse_event.row));
                if over_inspector {
                    self.scroll_inspector(3);
                } else {
                    self.scroll_viewport(3);
                    self.sync_inspector_to_cursor();
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(columns) = self.last_columns else {
                    return;
                };

                if columns
                    .inspector
                    .is_some_and(|area| rect_contains(area, mouse_event.column, mouse_event.row))
                {
                    if !self.mode.is_inspector() {
                        self.leave_mode();
                    }
                    self.mode = Mode::Inspector;
                }

                if let Some(hit) =
                    self.main_view_mouse_hit(columns, mouse_event.column, mouse_event.row)
                {
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
                                // Clicking a collapsible header toggles it — the
                                // keyboard path is Enter/Space; mouse users get
                                // the same affordance through the indicator click.
                                let clicked_collapsible_header = self
                                    .inspector
                                    .as_ref()
                                    .and_then(|inspector| inspector.rows.get(row_index))
                                    .is_some_and(|row| {
                                        matches!(
                                            row,
                                            InspectorRow::Header {
                                                has_children: true,
                                                ..
                                            }
                                        )
                                    });
                                if clicked_collapsible_header {
                                    self.toggle_inspector_collapse();
                                } else {
                                    self.sync_cursor_to_inspector();
                                }
                            }
                        }
                        return;
                    }

                    if self.mode.is_inspector() {
                        self.leave_mode();
                    }
                    if matches!(self.mode, Mode::InsertHex { .. }) {
                        self.commit_pending_insert();
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
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(columns) = self.last_columns else {
                    return;
                };
                let Some(hit) =
                    self.main_view_mouse_hit(columns, mouse_event.column, mouse_event.row)
                else {
                    return;
                };

                let anchor = self.mouse_selection_anchor.unwrap_or(hit.offset);
                self.selection_anchor = Some(anchor);
                self.cursor = hit.offset;
                self.mode = Mode::Visual;
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_selection_anchor = None;
            }
            _ => {}
        }
    }

    fn main_view_mouse_hit(
        &mut self,
        columns: crate::view::layout::MainColumns,
        x: u16,
        y: u16,
    ) -> Option<crate::input::mouse::MouseHit> {
        match &self.main_view {
            crate::app::MainView::Hex => crate::input::mouse::hit_test(
                columns,
                x,
                y,
                self.viewport_top,
                self.config.bytes_per_line,
                self.document.len(),
            ),
            crate::app::MainView::Disassembly(state) => {
                let state = state.clone();
                let rows = self
                    .collect_disassembly_rows(
                        &state,
                        state.viewport_top,
                        self.visible_rows() as usize,
                    )
                    .ok()?;
                crate::input::mouse::disassembly_hit_test(columns, x, y, &rows)
            }
        }
    }
}
