use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::app::App;
use crate::app::SidePanelKind;
use crate::format::parse::InspectorRow;
use crate::mode::{Mode, NibblePhase};
use crate::util::geometry::rect_contains;

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                let over_side_panel = self
                    .last_columns
                    .and_then(|columns| columns.side_panel)
                    .is_some_and(|area| rect_contains(area, mouse_event.column, mouse_event.row));
                if over_side_panel {
                    if self.active_side_panel == SidePanelKind::Symbol {
                        let visible_row = self
                            .last_columns
                            .and_then(|columns| columns.side_panel)
                            .map(|area| mouse_event.row.saturating_sub(area.y) as usize)
                            .unwrap_or(0);
                        if visible_row > self.symbol_list_visible_rows() {
                            let width = self
                                .last_columns
                                .and_then(|columns| columns.side_panel)
                                .map(|area| area.width)
                                .unwrap_or(1);
                            self.scroll_symbol_detail(-3, width);
                        } else {
                            self.scroll_symbol_panel(-3);
                        }
                    } else if self.active_side_panel == SidePanelKind::Data {
                        self.scroll_data_panel(-3);
                    } else if self.active_side_panel == SidePanelKind::Diff {
                        self.scroll_diff_panel(-3);
                    } else {
                        self.scroll_inspector(-3);
                    }
                } else {
                    self.scroll_viewport(-3);
                    self.sync_inspector_to_cursor();
                }
            }
            MouseEventKind::ScrollDown => {
                let over_side_panel = self
                    .last_columns
                    .and_then(|columns| columns.side_panel)
                    .is_some_and(|area| rect_contains(area, mouse_event.column, mouse_event.row));
                if over_side_panel {
                    if self.active_side_panel == SidePanelKind::Symbol {
                        let visible_row = self
                            .last_columns
                            .and_then(|columns| columns.side_panel)
                            .map(|area| mouse_event.row.saturating_sub(area.y) as usize)
                            .unwrap_or(0);
                        if visible_row > self.symbol_list_visible_rows() {
                            let width = self
                                .last_columns
                                .and_then(|columns| columns.side_panel)
                                .map(|area| area.width)
                                .unwrap_or(1);
                            self.scroll_symbol_detail(3, width);
                        } else {
                            self.scroll_symbol_panel(3);
                        }
                    } else if self.active_side_panel == SidePanelKind::Data {
                        self.scroll_data_panel(3);
                    } else if self.active_side_panel == SidePanelKind::Diff {
                        self.scroll_diff_panel(3);
                    } else {
                        self.scroll_inspector(3);
                    }
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
                    .side_panel
                    .is_some_and(|area| rect_contains(area, mouse_event.column, mouse_event.row))
                {
                    if !self.mode.is_side_panel() {
                        self.leave_mode();
                    }
                    self.mode = Mode::SidePanel;
                    if self.show_side_panel && self.active_side_panel == SidePanelKind::Diff {
                        if let Some(area) = columns.side_panel {
                            let visible_row = mouse_event.row.saturating_sub(area.y) as usize;
                            let panel_x = mouse_event.column.saturating_sub(area.x);
                            let col = crate::view::diff_panel::byte_col_from_x(
                                panel_x,
                                crate::util::format::offset_width(self.document.len()),
                                self.config.bytes_per_line,
                            )
                            .unwrap_or(0);
                            if let Some(hit) = self.visible_diff_cell_hit(
                                visible_row,
                                col,
                                crate::input::mouse::DiffCellSide::Other,
                            ) {
                                let anchor = hit
                                    .current_display_offset
                                    .or_else(|| {
                                        self.document
                                            .display_offset_for_logical_offset(hit.visual_offset)
                                    })
                                    .unwrap_or_else(|| self.clamp_offset(hit.visual_offset));
                                self.cursor = self.clamp_offset(anchor);
                                if let Some(other_offset) = hit.other_offset {
                                    self.select_diff_other_cell(other_offset, self.cursor);
                                } else {
                                    self.clear_diff_cell_selection();
                                }
                                self.ensure_cursor_visible();
                                self.sync_inspector_to_cursor();
                                self.refresh_data_panel();
                            } else {
                                self.select_diff_panel_row(visible_row);
                            }
                        }
                        return;
                    }
                }

                if let Some(hit) =
                    self.main_view_mouse_hit(columns, mouse_event.column, mouse_event.row)
                {
                    if let Some(visible_row) = hit.side_panel_row {
                        if self.show_side_panel {
                            self.mode = Mode::SidePanel;
                        }
                        if self.show_side_panel && self.active_side_panel == SidePanelKind::Symbol {
                            if visible_row >= self.symbol_list_visible_rows() {
                                return;
                            }
                            let actual_row = self
                                .symbol_state()
                                .map(|state| state.scroll_offset + visible_row)
                                .unwrap_or(visible_row);
                            self.set_symbol_selected_row(actual_row);
                            if let Err(error) = self.navigate_to_selected_symbol() {
                                self.set_error_status(error.to_string());
                            }
                            return;
                        }
                        if self.show_side_panel && self.active_side_panel == SidePanelKind::Data {
                            let actual_row = self
                                .data_state()
                                .map(|state| state.scroll_offset + visible_row + 1)
                                .unwrap_or(visible_row + 1);
                            self.select_data_panel_row(actual_row);
                            return;
                        }
                        if self.show_side_panel && self.active_side_panel == SidePanelKind::Diff {
                            return;
                        }
                        if self.show_side_panel
                            && self.active_side_panel == SidePanelKind::Inspector
                            && self.inspector().is_some()
                        {
                            let width = columns.side_panel.map(|area| area.width).unwrap_or(32);
                            let actual_visual_row = self
                                .inspector()
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
                                    .inspector()
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

                    if self.mode.is_side_panel() {
                        self.leave_mode();
                    }
                    if matches!(self.mode, Mode::DisasmEdit) {
                        self.cancel_disasm_edit();
                    }
                    if matches!(self.mode, Mode::InsertHex { .. }) {
                        self.commit_pending_insert();
                    }
                    let Some(selection_offset) = self.resolve_mouse_hit_offset(hit) else {
                        return;
                    };
                    self.mouse_selection_anchor = Some(selection_offset);
                    self.cursor = selection_offset;
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
                        Mode::Normal | Mode::SidePanel | Mode::InspectorEdit | Mode::DisasmEdit => {
                        }
                    }
                    self.ensure_cursor_visible();
                    self.sync_inspector_to_cursor();
                    self.refresh_data_panel();
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

                let Some(selection_offset) = self.resolve_mouse_hit_offset(hit) else {
                    return;
                };
                let anchor = self.mouse_selection_anchor.unwrap_or(selection_offset);
                self.selection_anchor = Some(anchor);
                self.cursor = selection_offset;
                self.mode = Mode::Visual;
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
                self.refresh_data_panel();
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
            crate::app::MainView::Hex => {
                if self.diff_projection_active() {
                    crate::input::mouse::hit_test_diff_projected(
                        columns,
                        x,
                        y,
                        self.viewport_top,
                        self.config.bytes_per_line,
                        self.document.len().max(
                            self.viewport_top.saturating_add(
                                self.visible_rows()
                                    .saturating_mul(self.config.bytes_per_line as u64),
                            ),
                        ),
                    )
                } else {
                    crate::input::mouse::hit_test(
                        columns,
                        x,
                        y,
                        self.viewport_top,
                        self.config.bytes_per_line,
                        self.document.len(),
                    )
                }
            }
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

    fn resolve_mouse_hit_offset(&mut self, hit: crate::input::mouse::MouseHit) -> Option<u64> {
        let Some(side) = hit.diff_side else {
            self.clear_diff_cell_selection();
            return Some(hit.offset);
        };
        let visible_row = hit
            .offset
            .saturating_sub(self.viewport_top)
            .checked_div(self.config.bytes_per_line as u64)
            .unwrap_or(0) as usize;
        let col = hit
            .offset
            .saturating_sub(self.viewport_top)
            .checked_rem(self.config.bytes_per_line as u64)
            .unwrap_or(0) as usize;
        let Some(diff_hit) = self.visible_diff_cell_hit(visible_row, col, side) else {
            self.clear_diff_cell_selection();
            return None;
        };
        match diff_hit.side {
            crate::input::mouse::DiffCellSide::Current => {
                self.clear_diff_cell_selection();
                let target = diff_hit
                    .current_display_offset
                    .unwrap_or(diff_hit.visual_offset);
                Some(self.clamp_offset(target))
            }
            crate::input::mouse::DiffCellSide::Other => {
                let anchor = diff_hit
                    .current_display_offset
                    .or_else(|| {
                        self.document
                            .display_offset_for_logical_offset(diff_hit.visual_offset)
                    })
                    .unwrap_or_else(|| self.clamp_offset(diff_hit.visual_offset));
                if let Some(other_offset) = diff_hit.other_offset {
                    self.select_diff_other_cell(other_offset, anchor);
                } else {
                    self.clear_diff_cell_selection();
                }
                Some(self.clamp_offset(anchor))
            }
        }
    }
}
