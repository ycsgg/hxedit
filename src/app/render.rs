use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::helpers::separator_widget;
use crate::app::App;
use crate::commands::hints;
use crate::core::document::ByteSlot;
use crate::mode::Mode;
use crate::profile::{FrameStats, RenderMainStats};
use crate::util::format::offset_width;
use crate::view::{
    ascii_grid, command_line, gutter, hex_grid, inspector as inspector_view, layout, status,
};

impl App {
    pub(crate) fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        let profiling = self.profiler.is_some();
        let frame_start = profiling.then(Instant::now);
        let screen = layout::split_screen(frame.area(), self.mode == Mode::Command);
        self.last_command_area = screen.command;
        let main_start = profiling.then(Instant::now);
        let main_stats = self.render_main(frame, screen.main, profiling);
        let main_elapsed = main_start.map(|start| start.elapsed()).unwrap_or_default();
        let status_start = profiling.then(Instant::now);
        self.render_status(frame, screen.status);
        let status_elapsed = status_start
            .map(|start| start.elapsed())
            .unwrap_or_default();
        let command_start = profiling.then(Instant::now);
        if let Some(command_area) = screen.command {
            self.render_command(frame, command_area);
        }
        let command_elapsed = command_start
            .map(|start| start.elapsed())
            .unwrap_or_default();
        if let (Some(start), Some(profiler)) = (frame_start, self.profiler.as_mut()) {
            profiler.record_frame(
                FrameStats {
                    total: start.elapsed(),
                    main: main_elapsed,
                    status: status_elapsed,
                    command: command_elapsed,
                    main_stats,
                },
                self.document.io_stats(),
            );
        }
    }

    pub(crate) fn render_main(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        profiling: bool,
    ) -> RenderMainStats {
        let mut stats = RenderMainStats::default();
        let block = Block::default().borders(Borders::ALL);
        let columns = layout::split_main(
            &block,
            area,
            offset_width(self.document.len()) as u16,
            self.show_inspector,
        );
        self.last_columns = Some(columns);
        frame.render_widget(block, area);

        // Keep render-derived row count in sync with navigation and paging.
        self.view_rows = columns.gutter.height.max(1) as usize;
        stats.rows = self.view_rows;

        let row_collect_start = profiling.then(Instant::now);
        let row_count = columns.gutter.height as usize;
        let mut row_offsets = Vec::with_capacity(row_count);
        let mut rows = Vec::with_capacity(row_count);
        for row in 0..row_count {
            let offset = self.viewport_top + row as u64 * self.config.bytes_per_line as u64;
            row_offsets.push(offset);
            let row_data = self
                .document
                .row_bytes(offset, self.config.bytes_per_line)
                .unwrap_or_else(|_| vec![ByteSlot::Empty; self.config.bytes_per_line]);
            rows.push(row_data);
        }
        if let Some(start) = row_collect_start {
            stats.row_collect = start.elapsed();
        }

        let line_build_start = profiling.then(Instant::now);
        let gutter_lines = if self.document.is_empty() {
            vec![Line::raw("No data")]
        } else {
            gutter::build(
                &row_offsets,
                offset_width(self.document.len()),
                &self.palette,
            )
        };
        let hex_lines = if self.document.is_empty() {
            vec![Line::raw("No content")]
        } else {
            hex_grid::build(
                &rows,
                &row_offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
                self.selection_range(),
            )
        };
        let ascii_lines = if self.document.is_empty() {
            vec![Line::raw("")]
        } else {
            ascii_grid::build(
                &rows,
                &row_offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
                self.selection_range(),
            )
        };
        if let Some(start) = line_build_start {
            stats.line_build = start.elapsed();
        }

        let widget_draw_start = profiling.then(Instant::now);
        frame.render_widget(Paragraph::new(gutter_lines), columns.gutter);
        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            columns.sep1,
        );
        frame.render_widget(
            Paragraph::new(hex_lines).wrap(Wrap { trim: false }),
            columns.hex,
        );
        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            columns.sep2,
        );
        frame.render_widget(Paragraph::new(ascii_lines), columns.ascii);

        // Render inspector panel (if present)
        if let (Some(sep3), Some(inspector_area)) = (columns.sep3, columns.inspector) {
            frame.render_widget(separator_widget(columns.gutter.height, &self.palette), sep3);
            if let Some(insp) = &self.inspector {
                let editing = insp
                    .editing
                    .as_ref()
                    .map(|e| (e.buffer.as_str(), e.cursor_pos));
                let all_lines = inspector_view::build_wrapped(
                    &insp.rows,
                    insp.selected_row,
                    editing,
                    inspector_area.width,
                    &self.palette,
                );
                let visible_height = inspector_area.height.saturating_sub(1) as usize;
                let visible_start = insp.scroll_offset.min(all_lines.len());
                let visible_end = (visible_start + visible_height).min(all_lines.len());
                let mut lines = vec![Line::styled(
                    format!("Format: {}", insp.format_name),
                    self.palette.inspector_header,
                )];
                lines.extend(
                    all_lines[visible_start..visible_end]
                        .iter()
                        .map(|line| line.line.clone()),
                );
                frame.render_widget(Paragraph::new(lines), inspector_area);

                if self.mode == Mode::InspectorEdit {
                    if let Some((visible_row, cursor_col)) =
                        all_lines.iter().enumerate().find_map(|(visual_idx, line)| {
                            (visual_idx >= visible_start && line.cursor_col.is_some())
                                .then(|| (visual_idx - visible_start, line.cursor_col.unwrap_or(0)))
                        })
                    {
                        if visible_row < self.inspector_visible_rows() {
                            frame.set_cursor_position((
                                inspector_area.x + cursor_col,
                                inspector_area.y + 1 + visible_row as u16,
                            ));
                        }
                    }
                }
            } else {
                frame.render_widget(Paragraph::new("No format detected"), inspector_area);
            }
        }

        if let Some(start) = widget_draw_start {
            stats.widget_draw = start.elapsed();
        }
        stats
    }

    pub(crate) fn render_status(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let path_display = self.document.path().to_string_lossy();
        let line = status::build(
            status::StatusInfo {
                mode: self.mode,
                path: &path_display,
                cursor: self.cursor,
                len: self.document.len(),
                selection_len: self.selection_range().map(|(start, end)| end - start + 1),
                paste_info: self.last_paste.as_ref().map(|state| state.summary.as_str()),
                dirty: self.document.is_dirty(),
                message: &self.status_message,
                readonly: self.document.is_readonly(),
            },
            &self.palette,
        );
        frame.render_widget(Paragraph::new(line), area);
    }

    pub(crate) fn render_command(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let hint = hints::hint_for(&self.command_buffer);
        let widget = command_line::widget(&self.command_buffer, hint, &self.palette);
        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        frame.render_widget(widget, area);
        let cursor_cols = self.command_buffer
            [..self.command_cursor_pos.min(self.command_buffer.len())]
            .chars()
            .count() as u16;
        frame.set_cursor_position((inner.x + 1 + cursor_cols, inner.y));
    }
}
