use super::*;

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
            crate::app::MainView::Hex => {
                offset_width(
                    self.memory_base_va()
                        .and_then(|base| base.checked_add(self.document.len().saturating_sub(1)))
                        .unwrap_or_else(|| self.document.len()),
                ) as u16
                    + 2
            }
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
        let side_panel_policy =
            if self.show_side_panel && self.active_side_panel == SidePanelKind::Diff {
                layout::SidePanelWidthPolicy::Half
            } else {
                layout::SidePanelWidthPolicy::Normal
            };
        let columns = layout::split_main(
            &block,
            area,
            gutter_width,
            self.show_side_panel,
            main_pane_kind,
            side_panel_policy,
        );
        self.last_columns = Some(columns);
        self.last_main_pane_kind = columns.main_pane_kind;
        self.ensure_side_panel_focus_visible();
        frame.render_widget(block, area);

        self.view_rows = match main_pane_kind {
            layout::MainPaneKind::Hex => columns.gutter.height.saturating_sub(1).max(1) as usize,
            layout::MainPaneKind::Disassembly => columns.gutter.height.max(1) as usize,
        };
        if self.show_side_panel && self.active_side_panel == SidePanelKind::Bookmarks {
            self.ensure_bookmark_selection_visible();
        }
        stats.rows = self.view_rows;

        let line_build_start = profiling.then(Instant::now);
        let main_lines = match &self.main_view {
            crate::app::MainView::Hex => {
                let row_collect_start = profiling.then(Instant::now);
                let visible_rows = self.collect_visible_rows(self.view_rows);
                if let Some(start) = row_collect_start {
                    stats.row_collect = start.elapsed();
                }
                self.build_hex_main_lines(&visible_rows)
            }
            crate::app::MainView::Disassembly(_) => self.build_disassembly_lines(self.view_rows),
        };
        if let Some(start) = line_build_start {
            stats.line_build = start.elapsed();
        }

        let widget_draw_start = profiling.then(Instant::now);
        let cached_diff_page = main_lines.diff_page.clone();
        self.render_main_grids(frame, columns, main_lines);
        self.render_side_panel(frame, columns, cached_diff_page.as_ref());

        if let Some(start) = widget_draw_start {
            stats.widget_draw = start.elapsed();
        }
        stats
    }

    pub(crate) fn render_status(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let path_display = self.document.label();
        let (selection_span, selection_logical_len) = match self.selection_range() {
            Some((start, end)) => {
                let logical_len = match self.document.logical_byte_count(start, end) {
                    Ok(count) => Some(count),
                    Err(err) => {
                        self.report_render_error(format!(
                            "status selection count failed at 0x{start:x}..0x{end:x}: {err}"
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
                cursor_label: self
                    .display_offset_to_va(self.cursor)
                    .map(|va| format!("va 0x{va:x}")),
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
            area.width,
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

    pub(super) fn report_render_error(&mut self, message: String) {
        if self.last_render_error.as_deref() != Some(message.as_str()) {
            eprintln!("{message}");
        }
        self.last_render_error = Some(message.clone());
        if self.status_message.is_empty() {
            self.set_error_status(message);
        }
    }

    pub(super) fn collect_visible_rows(&mut self, row_count: usize) -> VisibleRows {
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

    fn build_hex_main_lines(&mut self, visible_rows: &VisibleRows) -> MainLines {
        if self.document.is_empty() {
            return MainLines {
                gutter: vec![Line::raw("No data")],
                pane: MainPaneLines::Hex {
                    hex_header: hex_grid::column_header(self.config.bytes_per_line, &self.palette),
                    hex: vec![Line::raw("No content")],
                    ascii_header: Line::raw(""),
                    ascii: vec![Line::raw("")],
                },
                diff_page: None,
            };
        }

        let selection = self.selection_range();
        let inspector_highlight = self.inspector_highlight_range();
        let search_matches = self.visible_search_matches(visible_rows);
        let bookmarks = self.visible_bookmark_ranges(visible_rows);
        let diff_page = self.visible_diff_page(visible_rows).unwrap_or_else(|err| {
            self.report_render_error(format!("diff render failed: {err}"));
            VisibleDiffPage::default()
        });
        let cached_diff_page = self.diff_projection_active().then(|| diff_page.clone());
        let projected = !diff_page.main_rows.is_empty();
        let gutter_offsets = if projected {
            &diff_page.main_row_offsets
        } else {
            &visible_rows.offsets
        };
        let gutter_bookmarks = self.visible_bookmark_gutter_markers(gutter_offsets);
        let gutter_display_offsets;
        let (gutter_offsets, gutter_width_value) = if let Some(base_va) = self.memory_base_va() {
            gutter_display_offsets = gutter_offsets
                .iter()
                .map(|offset| base_va.saturating_add(*offset))
                .collect::<Vec<_>>();
            let max_va = base_va.saturating_add(self.document.len().saturating_sub(1));
            (&gutter_display_offsets[..], offset_width(max_va))
        } else {
            (gutter_offsets.as_slice(), offset_width(self.document.len()))
        };
        let gutter_lines = gutter::build_with_bookmarks(
            gutter_offsets,
            gutter_width_value,
            &gutter_bookmarks,
            &self.palette,
        );
        let ascii_bookmarks = bookmarks.clone();
        let ascii_overlays = ascii_grid::AsciiGridOverlays {
            selection,
            bookmarks: &ascii_bookmarks,
        };
        let overlays = hex_grid::HexGridOverlays {
            diff_spans: diff_page.overlay_spans.clone(),
            selection,
            inspector_highlight,
            search_matches,
            bookmarks,
        };
        let (hex, ascii) = if projected {
            (
                hex_grid::build_projected(
                    &diff_page.main_rows,
                    self.cursor,
                    self.mode,
                    &self.palette,
                    self.config.bytes_per_line,
                    overlays,
                ),
                ascii_grid::build_projected(
                    &diff_page.main_ascii_rows,
                    self.cursor,
                    self.mode,
                    &self.palette,
                    self.config.bytes_per_line,
                    ascii_overlays,
                ),
            )
        } else {
            (
                hex_grid::build(
                    &visible_rows.rows,
                    &visible_rows.offsets,
                    self.cursor,
                    self.mode,
                    &self.palette,
                    self.config.bytes_per_line,
                    overlays,
                ),
                ascii_grid::build(
                    &visible_rows.rows,
                    &visible_rows.offsets,
                    self.cursor,
                    self.mode,
                    &self.palette,
                    self.config.bytes_per_line,
                    ascii_overlays,
                ),
            )
        };
        MainLines {
            gutter: gutter_lines,
            pane: MainPaneLines::Hex {
                hex_header: hex_grid::column_header(self.config.bytes_per_line, &self.palette),
                hex,
                ascii_header: Line::raw("ASCII"),
                ascii,
            },
            diff_page: cached_diff_page,
        }
    }

    pub(super) fn build_disassembly_lines(&mut self, row_count: usize) -> MainLines {
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
        let rows =
            cache.collect_rows(&mut self.document, &state.info, backend, start, row_count)?;
        #[cfg(feature = "sagitta-analysis")]
        {
            let mut rows = rows;
            self.apply_sagitta_annotations(&mut rows);
            Ok(rows)
        }
        #[cfg(not(feature = "sagitta-analysis"))]
        {
            Ok(rows)
        }
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
                    jump_rail: None,
                },
                diff_page: None,
            };
        }

        let cursor = self.cursor_anchor_offset();
        let editing = self
            .disasm_edit()
            .map(|edit| (edit.row_offset, edit.buffer.as_str()));
        let rail_width = self
            .last_columns
            .and_then(|columns| self.disassembly_jump_rail_width(columns))
            .filter(|_| rows.iter().any(|row| row.direct_target.is_some()));
        let ascii_width = self
            .last_columns
            .map(|columns| columns.ascii.width as usize)
            .unwrap_or(80);
        let text_width = self
            .last_columns
            .map(|_| ascii_width.saturating_sub(rail_width.unwrap_or(0)))
            .unwrap_or(80);
        let display = disasm_grid::build_display(
            rows,
            gutter_width,
            cursor,
            editing,
            text_width,
            &self.palette,
        );
        let jump_rail = rail_width.map(|width| {
            disasm_grid::build_jump_rail(
                rows,
                &display.row_sources,
                &display.text,
                ascii_width.max(width),
                &self.palette,
            )
        });
        let text = if let Some(jump_rail) = jump_rail.as_ref() {
            disasm_grid::merge_jump_rail(display.text, jump_rail, &self.palette)
        } else {
            display.text
        };
        MainLines {
            gutter: display.gutter,
            pane: MainPaneLines::Disassembly {
                bytes: display.bytes,
                text,
                jump_rail,
            },
            diff_page: None,
        }
    }

    fn disassembly_error_lines(&self, message: &str) -> MainLines {
        MainLines {
            gutter: vec![Line::raw("ERR")],
            pane: MainPaneLines::Disassembly {
                bytes: vec![Line::styled("--", self.palette.separator)],
                text: vec![Line::styled(message.to_owned(), self.palette.error)],
                jump_rail: None,
            },
            diff_page: None,
        }
    }

    pub(crate) fn disassembly_jump_rail_width(
        &self,
        columns: layout::MainColumns,
    ) -> Option<usize> {
        if self.show_side_panel && self.active_side_panel == SidePanelKind::Symbol {
            return None;
        }
        let width = columns.ascii.width as usize;
        (width >= disasm_grid::JUMP_RAIL_MIN_TEXT_WIDTH + disasm_grid::JUMP_RAIL_WIDTH)
            .then_some(disasm_grid::JUMP_RAIL_WIDTH)
    }

    fn set_disassembly_render_error(&mut self, error: Option<String>) {
        if let crate::app::MainView::Disassembly(state) = &mut self.main_view {
            state.last_error = error;
        }
    }

    pub(super) fn visible_search_matches(&self, visible_rows: &VisibleRows) -> Vec<(u64, u64)> {
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

    pub(super) fn visible_bookmark_ranges(
        &self,
        visible_rows: &VisibleRows,
    ) -> Vec<hex_grid::BookmarkOverlay> {
        let Some(first) = visible_rows.offsets.first().copied() else {
            return Vec::new();
        };
        let rows = visible_rows.offsets.len() as u64;
        let last = first
            .saturating_add(rows.saturating_mul(self.config.bytes_per_line as u64))
            .saturating_sub(1);
        self.bookmark_state
            .entries
            .iter()
            .filter_map(|entry| {
                let start = entry.start.max(first);
                let end = entry.end().min(last);
                (start <= end).then_some(hex_grid::BookmarkOverlay {
                    start,
                    end,
                    color_index: entry.color.palette_index(),
                })
            })
            .collect()
    }

    pub(super) fn visible_bookmark_gutter_markers(
        &self,
        row_offsets: &[u64],
    ) -> Vec<gutter::BookmarkGutterMarker> {
        let row_size = self.config.bytes_per_line as u64;
        row_offsets
            .iter()
            .map(|row_start| {
                let row_end = row_start.saturating_add(row_size.saturating_sub(1));
                let mut marker = gutter::BookmarkGutterMarker::None;
                for entry in &self.bookmark_state.entries {
                    if entry.start <= row_end && entry.end() >= *row_start {
                        let color = entry.color.palette_index();
                        marker = if entry.note.is_some() {
                            gutter::BookmarkGutterMarker::Note(color)
                        } else {
                            gutter::BookmarkGutterMarker::Bookmark(color)
                        };
                        if entry.note.is_some() {
                            break;
                        }
                    }
                }
                marker
            })
            .collect()
    }

    fn render_main_grids(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        columns: layout::MainColumns,
        lines: MainLines,
    ) {
        let is_hex = matches!(&lines.pane, MainPaneLines::Hex { .. });
        let (gutter_area, hex_area, ascii_area) = if is_hex {
            (
                scrolled_body_area(columns.gutter),
                scrolled_body_area(columns.hex),
                scrolled_body_area(columns.ascii),
            )
        } else {
            (columns.gutter, columns.hex, columns.ascii)
        };

        frame.render_widget(Paragraph::new(lines.gutter), gutter_area);
        frame.render_widget(
            separator_widget(columns.sep1.height, &self.palette),
            columns.sep1,
        );
        match lines.pane {
            MainPaneLines::Hex {
                hex_header,
                hex,
                ascii_header,
                ascii,
            } => {
                let header_area = top_header_area(columns.hex);
                if header_area.height > 0 {
                    frame.render_widget(Paragraph::new(hex_header), header_area);
                }
                frame.render_widget(Paragraph::new(hex).wrap(Wrap { trim: false }), hex_area);
                let ascii_header_area = top_header_area(columns.ascii);
                if ascii_header_area.height > 0 {
                    frame.render_widget(Paragraph::new(ascii_header), ascii_header_area);
                }
                frame.render_widget(
                    separator_widget(columns.sep2.height, &self.palette),
                    columns.sep2,
                );
                frame.render_widget(Paragraph::new(ascii), ascii_area);
            }
            MainPaneLines::Disassembly {
                bytes,
                text,
                jump_rail,
            } => {
                let _has_jump_rail = jump_rail.as_ref().is_some_and(|rail| !rail.is_empty());
                frame.render_widget(Paragraph::new(bytes), hex_area);
                frame.render_widget(
                    separator_widget(columns.sep2.height, &self.palette),
                    columns.sep2,
                );
                frame.render_widget(Paragraph::new(text), ascii_area);
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
        let row_index = match rows.iter().position(|row| row.offset == row_offset) {
            Some(index) => index,
            None => return,
        };
        let editing = self
            .disasm_edit()
            .map(|edit| (edit.row_offset, edit.buffer.as_str()));
        let rail_width = self.disassembly_jump_rail_width(columns).unwrap_or(0);
        let text_width = columns.ascii.width.saturating_sub(rail_width as u16);
        let display = disasm_grid::build_display(
            &rows,
            columns.gutter.width as usize,
            self.cursor_anchor_offset(),
            editing,
            text_width as usize,
            &self.palette,
        );
        let visible_row = match display
            .row_sources
            .iter()
            .position(|source| *source == Some(row_index))
        {
            Some(row) => row,
            None => return,
        };
        let Some(edit) = self.disasm_edit() else {
            return;
        };
        let cursor_col = edit.buffer[..cursor_pos.min(buffer_len)].chars().count() as u16;
        if visible_row < columns.ascii.height as usize && cursor_col < text_width {
            frame.set_cursor_position((
                columns.ascii.x + cursor_col,
                columns.ascii.y + visible_row as u16,
            ));
        }
    }
}
