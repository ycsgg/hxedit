use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, MainView, SidePanel};
use crate::commands::hints;
use crate::core::document::ByteSlot;
use crate::disasm::DisasmRow;
use crate::mode::Mode;
use crate::profile::{FrameStats, RenderMainStats};
use crate::util::format::offset_width;
use crate::view::{
    ascii_grid, command_line, data_panel, disasm_grid, gutter, hex_grid,
    inspector as inspector_view, layout, status, symbol_panel,
};

struct VisibleRows {
    offsets: Vec<u64>,
    rows: Vec<Vec<ByteSlot>>,
}

enum MainPaneLines {
    Hex {
        hex: Vec<Line<'static>>,
        ascii: Vec<Line<'static>>,
    },
    Disassembly {
        bytes: Vec<Line<'static>>,
        text: Vec<Line<'static>>,
    },
}

struct MainLines {
    gutter: Vec<Line<'static>>,
    pane: MainPaneLines,
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
        let main_pane_kind = match &self.main_view {
            crate::app::MainView::Hex => layout::MainPaneKind::Hex,
            crate::app::MainView::Disassembly(_) => layout::MainPaneKind::Disassembly,
        };
        let gutter_width = match &self.main_view {
            crate::app::MainView::Hex => offset_width(self.document.len()) as u16,
            crate::app::MainView::Disassembly(state) => {
                let max_name = state
                    .info
                    .code_spans
                    .iter()
                    .filter_map(|span| span.name.as_ref().map(|name| name.chars().count()))
                    .max()
                    .unwrap_or(5) as u16;
                (max_name + offset_width(self.document.len()) as u16 + 4).clamp(14, 28)
            }
        };
        let columns = layout::split_main(
            &block,
            area,
            gutter_width,
            self.show_inspector,
            main_pane_kind,
        );
        self.last_columns = Some(columns);
        self.last_main_pane_kind = columns.main_pane_kind;
        self.ensure_inspector_mode_visible();
        frame.render_widget(block, area);

        self.view_rows = columns.gutter.height.max(1) as usize;
        stats.rows = self.view_rows;

        let line_build_start = profiling.then(Instant::now);
        let main_lines = match &self.main_view {
            crate::app::MainView::Hex => {
                let row_collect_start = profiling.then(Instant::now);
                let visible_rows = self.collect_visible_rows(columns.gutter.height as usize);
                if let Some(start) = row_collect_start {
                    stats.row_collect = start.elapsed();
                }
                self.build_hex_main_lines(&visible_rows)
            }
            crate::app::MainView::Disassembly(_) => {
                self.build_disassembly_lines(columns.gutter.height as usize)
            }
        };
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
                main_view_label: match &self.main_view {
                    crate::app::MainView::Hex => None,
                    crate::app::MainView::Disassembly(_) => Some("DIS"),
                },
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

    fn build_hex_main_lines(&self, visible_rows: &VisibleRows) -> MainLines {
        if self.document.is_empty() {
            return MainLines {
                gutter: vec![Line::raw("No data")],
                pane: MainPaneLines::Hex {
                    hex: vec![Line::raw("No content")],
                    ascii: vec![Line::raw("")],
                },
            };
        }

        let gutter_lines = gutter::build(
            &visible_rows.offsets,
            offset_width(self.document.len()),
            &self.palette,
        );
        let selection = self.selection_range();
        let inspector_highlight = self.inspector_highlight_range();
        let search_matches = self.visible_search_matches(visible_rows);
        MainLines {
            gutter: gutter_lines,
            pane: MainPaneLines::Hex {
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
                        search_matches,
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
            },
        }
    }

    fn build_disassembly_lines(&mut self, row_count: usize) -> MainLines {
        let crate::app::MainView::Disassembly(state) = &self.main_view else {
            unreachable!("disassembly lines requested outside disassembly view");
        };
        let state = state.clone();
        match self.collect_disassembly_rows(&state, state.viewport_top, row_count) {
            Ok(rows) => {
                self.set_disassembly_render_error(None);
                self.main_lines_from_disassembly_rows(
                    &rows,
                    self.last_columns.map(|c| c.gutter.width).unwrap_or(18) as usize,
                )
            }
            Err(err) => {
                let message = err.to_string();
                self.set_disassembly_render_error(Some(message.clone()));
                self.report_render_error(format!("disassembly render failed: {message}"));
                self.disassembly_error_lines(&message)
            }
        }
    }

    pub(crate) fn collect_disassembly_rows(
        &mut self,
        state: &crate::disasm::DisassemblyState,
        start: u64,
        row_count: usize,
    ) -> crate::error::HxResult<Vec<DisasmRow>> {
        self.ensure_disassembly_backend(state)?;
        let doc_len = self.document.len();
        let cache = self
            .disasm_cache
            .get_or_insert_with(|| crate::disasm::DisasmCache::new(&state.info, doc_len));
        let backend = self
            .disasm_backend
            .as_deref()
            .expect("disassembly backend should be initialized");
        cache.collect_rows(&mut self.document, &state.info, backend, start, row_count)
    }

    fn main_lines_from_disassembly_rows(
        &self,
        rows: &[DisasmRow],
        gutter_width: usize,
    ) -> MainLines {
        if rows.is_empty() {
            return MainLines {
                gutter: vec![Line::raw("No code")],
                pane: MainPaneLines::Disassembly {
                    bytes: vec![Line::styled("--", self.palette.separator)],
                    text: vec![Line::styled(
                        "no decodable instructions in current view",
                        self.palette.separator,
                    )],
                },
            };
        }

        let cursor = self.cursor_anchor_offset();
        let editing = self
            .disasm_edit()
            .map(|edit| (edit.row_offset, edit.buffer.as_str()));
        MainLines {
            gutter: disasm_grid::build_gutter(rows, gutter_width, cursor, &self.palette),
            pane: MainPaneLines::Disassembly {
                bytes: disasm_grid::build_bytes(rows, cursor, &self.palette),
                text: disasm_grid::build_text(rows, cursor, editing, &self.palette),
            },
        }
    }

    fn disassembly_error_lines(&self, message: &str) -> MainLines {
        MainLines {
            gutter: vec![Line::raw("ERR")],
            pane: MainPaneLines::Disassembly {
                bytes: vec![Line::styled("--", self.palette.separator)],
                text: vec![Line::styled(message.to_owned(), self.palette.error)],
            },
        }
    }

    fn set_disassembly_render_error(&mut self, error: Option<String>) {
        if let crate::app::MainView::Disassembly(state) = &mut self.main_view {
            state.last_error = error;
        }
    }

    fn visible_search_matches(&self, visible_rows: &VisibleRows) -> Vec<(u64, u64)> {
        let Some(search) = self.last_search.as_ref() else {
            return Vec::new();
        };
        let Some(pattern) = search.byte_pattern() else {
            return Vec::new();
        };
        if pattern.is_empty() {
            return Vec::new();
        }

        let mut slots = Vec::new();
        for (row_idx, row) in visible_rows.rows.iter().enumerate() {
            let row_offset = visible_rows
                .offsets
                .get(row_idx)
                .copied()
                .unwrap_or_default();
            for (col_idx, slot) in row.iter().enumerate() {
                slots.push((row_offset + col_idx as u64, *slot));
            }
        }

        if slots.len() < pattern.len() {
            return Vec::new();
        }

        let mut matches = Vec::new();
        for start_idx in 0..=slots.len() - pattern.len() {
            let matched = pattern.iter().enumerate().all(|(idx, expected)| {
                matches!(slots[start_idx + idx].1, ByteSlot::Present(byte) if byte == *expected)
            });
            if matched {
                matches.push((slots[start_idx].0, slots[start_idx + pattern.len() - 1].0));
            }
        }
        matches
    }

    fn render_main_grids(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        columns: layout::MainColumns,
        lines: MainLines,
    ) {
        frame.render_widget(Paragraph::new(lines.gutter), columns.gutter);
        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            columns.sep1,
        );
        match lines.pane {
            MainPaneLines::Hex { hex, ascii } => {
                frame.render_widget(Paragraph::new(hex).wrap(Wrap { trim: false }), columns.hex);
                frame.render_widget(
                    separator_widget(columns.gutter.height, &self.palette),
                    columns.sep2,
                );
                frame.render_widget(Paragraph::new(ascii), columns.ascii);
            }
            MainPaneLines::Disassembly { bytes, text } => {
                frame.render_widget(
                    Paragraph::new(bytes).wrap(Wrap { trim: false }),
                    columns.hex,
                );
                frame.render_widget(
                    separator_widget(columns.gutter.height, &self.palette),
                    columns.sep2,
                );
                frame.render_widget(
                    Paragraph::new(text).wrap(Wrap { trim: false }),
                    columns.ascii,
                );
                self.render_disassembly_edit_cursor(frame, columns);
            }
        }
    }

    fn render_disassembly_edit_cursor(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        columns: layout::MainColumns,
    ) {
        if self.mode != Mode::DisasmEdit {
            return;
        }
        let Some((row_offset, cursor_pos, buffer_len)) = self
            .disasm_edit()
            .map(|edit| (edit.row_offset, edit.cursor_pos, edit.buffer.len()))
        else {
            return;
        };
        let MainView::Disassembly(state) = &self.main_view else {
            return;
        };
        let state = state.clone();
        let Ok(rows) =
            self.collect_disassembly_rows(&state, state.viewport_top, self.visible_rows() as usize)
        else {
            return;
        };
        let visible_row = match rows.iter().position(|row| row.offset == row_offset) {
            Some(row) => row,
            None => return,
        };
        let Some(edit) = self.disasm_edit() else {
            return;
        };
        let cursor_col = edit.buffer[..cursor_pos.min(buffer_len)].chars().count() as u16;
        if visible_row < columns.ascii.height as usize && cursor_col < columns.ascii.width {
            frame.set_cursor_position((
                columns.ascii.x + cursor_col,
                columns.ascii.y + visible_row as u16,
            ));
        }
    }

    fn render_inspector_panel(&self, frame: &mut ratatui::Frame<'_>, columns: layout::MainColumns) {
        let (Some(sep3), Some(inspector_area)) = (columns.sep3, columns.inspector) else {
            return;
        };

        frame.render_widget(separator_widget(columns.gutter.height, &self.palette), sep3);

        match &self.side_panel {
            Some(SidePanel::Inspector(inspector)) => {
                self.render_visible_inspector(frame, inspector_area, inspector);
            }
            Some(SidePanel::Symbol(state)) => {
                self.render_symbol_panel(frame, inspector_area, state);
            }
            Some(SidePanel::Data(state)) => {
                self.render_data_panel(frame, inspector_area, state);
            }
            None => {
                if let Some(error) = &self.inspector_error {
                    frame.render_widget(
                        Paragraph::new(error.clone()).wrap(Wrap { trim: false }),
                        inspector_area,
                    );
                } else {
                    frame.render_widget(
                        Paragraph::new(self.inspector_empty_panel_message())
                            .wrap(Wrap { trim: false }),
                        inspector_area,
                    );
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::App;
    use crate::cli::Cli;
    use crate::commands::types::Command;

    pub(super) fn app_with_bytes(bytes: &[u8]) -> App {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, bytes).unwrap();
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
        app.view_rows = 2;
        app
    }

    #[test]
    fn visible_search_matches_collects_all_hits_on_screen() {
        let mut app = app_with_bytes(b"aba xx aba");
        app.execute_command(Command::SearchAscii {
            pattern: b"aba".to_vec(),
            backward: false,
        })
        .unwrap();

        let visible_rows = app.collect_visible_rows(1);
        assert_eq!(
            app.visible_search_matches(&visible_rows),
            vec![(0, 2), (7, 9)]
        );
    }

    #[cfg(feature = "disasm-capstone")]
    #[test]
    fn disassembly_main_view_renders_decoded_instruction_lines() {
        let mut app = app_with_bytes(&{
            let mut bytes = vec![0_u8; 0x200];
            bytes[0..4].copy_from_slice(b"ELF");
            bytes[4] = 2;
            bytes[5] = 1;
            bytes[6] = 1;
            bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
            bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
            bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
            bytes[24..32].copy_from_slice(&0x100u64.to_le_bytes());
            bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
            bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
            bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
            bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
            let ph = 64usize;
            bytes[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
            bytes[ph + 4..ph + 8].copy_from_slice(&0x5u32.to_le_bytes());
            bytes[ph + 8..ph + 16].copy_from_slice(&0x100u64.to_le_bytes());
            bytes[ph + 32..ph + 40].copy_from_slice(&6u64.to_le_bytes());
            bytes[0x100..0x106].copy_from_slice(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
            bytes
        });
        app.execute_command(Command::Disassemble { arch: None })
            .unwrap();
        let lines = app.build_disassembly_lines(4);
        match lines.pane {
            super::MainPaneLines::Disassembly { text, .. } => {
                let joined = text
                    .iter()
                    .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
                    .collect::<String>();
                assert!(joined.contains("push"));
                assert!(joined.contains("mov"));
                assert!(joined.contains("ret"));
            }
            _ => panic!("expected disassembly pane"),
        }
    }
}
