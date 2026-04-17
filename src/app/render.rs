use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::commands::hints;
use crate::core::document::ByteSlot;
use crate::mode::Mode;
use crate::profile::{FrameStats, RenderMainStats};
use crate::util::format::offset_width;
use crate::view::{
    ascii_grid, command_line, gutter, hex_grid, inspector as inspector_view, layout, status,
};

struct VisibleRows {
    offsets: Vec<u64>,
    rows: Vec<Vec<ByteSlot>>,
}

struct MainLines {
    gutter: Vec<Line<'static>>,
    hex: Vec<Line<'static>>,
    ascii: Vec<Line<'static>>,
}

fn separator_widget(height: u16, palette: &crate::view::palette::Palette) -> Paragraph<'static> {
    let lines = (0..height)
        .map(|_| Line::styled("│", palette.separator))
        .collect::<Vec<_>>();
    Paragraph::new(lines)
}

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
        self.ensure_inspector_mode_visible();
        frame.render_widget(block, area);

        // Keep render-derived row count in sync with navigation and paging.
        self.view_rows = columns.gutter.height.max(1) as usize;
        stats.rows = self.view_rows;

        let row_collect_start = profiling.then(Instant::now);
        let visible_rows = self.collect_visible_rows(columns.gutter.height as usize);
        if let Some(start) = row_collect_start {
            stats.row_collect = start.elapsed();
        }

        let line_build_start = profiling.then(Instant::now);
        let main_lines = self.build_main_lines(&visible_rows);
        if let Some(start) = line_build_start {
            stats.line_build = start.elapsed();
        }

        let widget_draw_start = profiling.then(Instant::now);
        self.render_main_grids(frame, columns, main_lines);
        self.render_inspector_panel(frame, columns);

        if let Some(start) = widget_draw_start {
            stats.widget_draw = start.elapsed();
        }
        stats
    }

    pub(crate) fn render_status(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let path_display = self.document.path().to_string_lossy().into_owned();
        let (selection_span, selection_logical_len) = match self.selection_range() {
            Some((start, end)) => {
                let logical_len = match self.document.logical_bytes(start, end) {
                    Ok(bytes) => Some(bytes.len() as u64),
                    Err(err) => {
                        self.report_render_error(format!(
                            "status selection read failed at 0x{start:x}..0x{end:x}: {err}"
                        ));
                        None
                    }
                };
                (Some(end - start + 1), logical_len)
            }
            None => (None, None),
        };
        let line = status::build(
            status::StatusInfo {
                mode: self.mode,
                path: &path_display,
                cursor: self.cursor,
                display_len: self.document.len(),
                visible_len: self.document.visible_len(),
                selection_span,
                selection_logical_len,
                paste_info: self.last_paste.as_ref().map(|state| state.summary.as_str()),
                dirty: self.document.is_dirty(),
                message: &self.status_message,
                message_level: self.status_level,
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

    fn report_render_error(&mut self, message: String) {
        if self.last_render_error.as_deref() != Some(message.as_str()) {
            eprintln!("{message}");
        }
        self.last_render_error = Some(message.clone());
        if self.status_message.is_empty() {
            self.set_error_status(message);
        }
    }

    fn collect_visible_rows(&mut self, row_count: usize) -> VisibleRows {
        let mut offsets = Vec::with_capacity(row_count);
        let mut rows = Vec::with_capacity(row_count);
        let mut saw_row_error = false;

        for row in 0..row_count {
            let offset = self.viewport_top + row as u64 * self.config.bytes_per_line as u64;
            offsets.push(offset);
            let row_data = match self.document.row_bytes(offset, self.config.bytes_per_line) {
                Ok(row_data) => row_data,
                Err(err) => {
                    saw_row_error = true;
                    self.report_render_error(format!("render read failed at 0x{offset:x}: {err}"));
                    vec![ByteSlot::Empty; self.config.bytes_per_line]
                }
            };
            rows.push(row_data);
        }

        if !saw_row_error {
            self.last_render_error = None;
        }

        VisibleRows { offsets, rows }
    }

    fn build_main_lines(&self, visible_rows: &VisibleRows) -> MainLines {
        if self.document.is_empty() {
            return MainLines {
                gutter: vec![Line::raw("No data")],
                hex: vec![Line::raw("No content")],
                ascii: vec![Line::raw("")],
            };
        }

        let selection = self.selection_range();
        let inspector_highlight = self.inspector_highlight_range();
        MainLines {
            gutter: gutter::build(
                &visible_rows.offsets,
                offset_width(self.document.len()),
                &self.palette,
            ),
            hex: hex_grid::build(
                &visible_rows.rows,
                &visible_rows.offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
                hex_grid::HexGridOverlays {
                    selection,
                    inspector_highlight,
                },
            ),
            ascii: ascii_grid::build(
                &visible_rows.rows,
                &visible_rows.offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
                selection,
            ),
        }
    }

    fn render_main_grids(
        &self,
        frame: &mut ratatui::Frame<'_>,
        columns: layout::MainColumns,
        lines: MainLines,
    ) {
        frame.render_widget(Paragraph::new(lines.gutter), columns.gutter);
        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            columns.sep1,
        );
        frame.render_widget(
            Paragraph::new(lines.hex).wrap(Wrap { trim: false }),
            columns.hex,
        );
        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            columns.sep2,
        );
        frame.render_widget(Paragraph::new(lines.ascii), columns.ascii);
    }

    fn render_inspector_panel(&self, frame: &mut ratatui::Frame<'_>, columns: layout::MainColumns) {
        let (Some(sep3), Some(inspector_area)) = (columns.sep3, columns.inspector) else {
            return;
        };

        frame.render_widget(separator_widget(columns.gutter.height, &self.palette), sep3);
        if let Some(insp) = &self.inspector {
            self.render_visible_inspector(frame, inspector_area, insp);
        } else if let Some(error) = &self.inspector_error {
            frame.render_widget(
                Paragraph::new(error.clone()).wrap(Wrap { trim: false }),
                inspector_area,
            );
        } else {
            frame.render_widget(
                Paragraph::new(self.inspector_empty_panel_message()).wrap(Wrap { trim: false }),
                inspector_area,
            );
        }
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
        let mut lines = vec![Line::styled(
            format!("Format: {}", inspector.format_name),
            self.palette.inspector_header,
        )];
        lines.extend(
            all_lines[visible_start..visible_end]
                .iter()
                .map(|line| line.line.clone()),
        );
        frame.render_widget(Paragraph::new(lines), inspector_area);
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

        if visible_row < self.inspector_visible_rows() {
            frame.set_cursor_position((
                inspector_area.x + cursor_col,
                inspector_area.y + 1 + visible_row as u16,
            ));
        }
    }
}
