use super::*;

impl App {
    pub(super) fn render_side_panel(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        columns: layout::MainColumns,
    ) {
        let (Some(side_panel_sep), Some(side_panel_area)) =
            (columns.side_panel_sep, columns.side_panel)
        else {
            return;
        };

        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            side_panel_sep,
        );

        match self.active_side_panel {
            SidePanelKind::Inspector => {
                if let Some(inspector) = self.inspector() {
                    self.render_visible_inspector(frame, side_panel_area, inspector);
                } else if let Some(error) = &self.inspector_error {
                    let header_area = top_header_area(side_panel_area);
                    if header_area.height > 0 {
                        frame.render_widget(
                            Paragraph::new(Line::styled(
                                "Inspector",
                                self.palette.inspector_header,
                            )),
                            header_area,
                        );
                    }
                    frame.render_widget(
                        Paragraph::new(error.clone()).wrap(Wrap { trim: false }),
                        scrolled_body_area(side_panel_area),
                    );
                } else {
                    let header_area = top_header_area(side_panel_area);
                    if header_area.height > 0 {
                        frame.render_widget(
                            Paragraph::new(Line::styled(
                                "Inspector",
                                self.palette.inspector_header,
                            )),
                            header_area,
                        );
                    }
                    frame.render_widget(
                        Paragraph::new(self.inspector_empty_panel_message())
                            .wrap(Wrap { trim: false }),
                        scrolled_body_area(side_panel_area),
                    );
                }
            }
            SidePanelKind::Symbol => {
                if let Some(state) = self.symbol_state() {
                    self.render_symbol_panel(frame, side_panel_area, state);
                }
            }
            SidePanelKind::Data => {
                if let Some(state) = self.data_state() {
                    self.render_data_panel(frame, side_panel_area, state);
                }
            }
            SidePanelKind::Diff => {
                self.render_diff_panel(frame, side_panel_area);
            }
        }
    }

    fn render_diff_panel(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        if !self.diff_projection_active() {
            return;
        }
        let body_area = scrolled_body_area(area);
        let visible_rows = self.collect_visible_rows(body_area.height as usize);
        let page = self.visible_diff_page(&visible_rows).unwrap_or_else(|err| {
            self.report_render_error(format!("diff panel render failed: {err}"));
            VisibleDiffPage::default()
        });
        let header = diff_panel::header_line(
            offset_width(self.document.len()),
            self.config.bytes_per_line,
            &self.palette,
        );
        let header_area = top_header_area(area);
        if header_area.height > 0 {
            frame.render_widget(Paragraph::new(header), header_area);
        }
        let lines = diff_panel::build_lines(
            &page.rows,
            offset_width(self.document.len()),
            self.config.bytes_per_line,
            &self.palette,
        );
        let visible_height = body_area.height as usize;
        let visible_end = visible_height.min(lines.len());
        frame.render_widget(Paragraph::new(lines[..visible_end].to_vec()), body_area);
    }

    fn render_data_panel(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        state: &crate::app::DataState,
    ) {
        let lines = data_panel::build_lines(state, area.width, &self.palette);
        let visible_height = area.height as usize;
        let visible_start = state.scroll_offset.min(lines.len());
        let visible_end = (visible_start + visible_height).min(lines.len());
        frame.render_widget(
            Paragraph::new(lines[visible_start..visible_end].to_vec()),
            area,
        );
    }

    fn render_symbol_panel(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        state: &crate::app::SymbolState,
    ) {
        let lines = symbol_panel::build_lines(state, state.selected_row, area.width, &self.palette);
        let list_height = symbol_panel::list_height(area.height);
        let visible_start = state.scroll_offset.min(lines.len());
        let visible_end = (visible_start + list_height.saturating_sub(1)).min(lines.len());

        let mut render_lines = vec![symbol_panel::header_line(area.width, &self.palette)];
        render_lines.extend(
            lines[visible_start..visible_end]
                .iter()
                .map(|line| line.line.clone()),
        );
        render_lines.push(Line::raw(""));
        let detail_lines = symbol_panel::detail_lines(state, area.width, &self.palette);
        let detail_height = symbol_panel::detail_height(area.height);
        let detail_start = state.detail_scroll_offset.min(detail_lines.len());
        let detail_end = (detail_start + detail_height).min(detail_lines.len());
        render_lines.extend(detail_lines[detail_start..detail_end].iter().cloned());

        frame.render_widget(Paragraph::new(render_lines), area);
    }

    fn render_visible_inspector(
        &self,
        frame: &mut ratatui::Frame<'_>,
        inspector_area: Rect,
        inspector: &crate::app::InspectorState,
    ) {
        let editing = inspector
            .editing
            .as_ref()
            .map(|edit| (edit.buffer.as_str(), edit.cursor_pos));
        let all_lines = inspector_view::build_wrapped(
            &inspector.rows,
            inspector.selected_row,
            editing,
            inspector_area.width,
            &self.palette,
        );
        let visible_height = inspector_area.height.saturating_sub(1) as usize;
        let visible_start = inspector.scroll_offset.min(all_lines.len());
        let visible_end = (visible_start + visible_height).min(all_lines.len());
        let header_area = top_header_area(inspector_area);
        if header_area.height > 0 {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!("Format: {}", inspector.format_name),
                    self.palette.inspector_header,
                )),
                header_area,
            );
        }
        let mut lines = Vec::new();
        lines.extend(
            all_lines[visible_start..visible_end]
                .iter()
                .map(|line| line.line.clone()),
        );
        frame.render_widget(Paragraph::new(lines), scrolled_body_area(inspector_area));
        self.render_inspector_edit_cursor(frame, inspector_area, &all_lines, visible_start);
    }

    fn render_inspector_edit_cursor(
        &self,
        frame: &mut ratatui::Frame<'_>,
        inspector_area: Rect,
        all_lines: &[inspector_view::RenderedInspectorLine],
        visible_start: usize,
    ) {
        if self.mode != Mode::InspectorEdit {
            return;
        }

        let Some((visible_row, cursor_col)) =
            all_lines.iter().enumerate().find_map(|(visual_idx, line)| {
                (visual_idx >= visible_start && line.cursor_col.is_some())
                    .then(|| (visual_idx - visible_start, line.cursor_col.unwrap_or(0)))
            })
        else {
            return;
        };

        if visible_row < self.side_panel_visible_rows() {
            frame.set_cursor_position((
                inspector_area.x + cursor_col,
                inspector_area.y + 1 + visible_row as u16,
            ));
        }
    }
}
