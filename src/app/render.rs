use std::collections::HashMap;
use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, MainView, SidePanelKind};
use crate::commands::hints;
use crate::core::document::ByteSlot;
use crate::diff::{diff_sources, DiffByte, DiffHunkKind, DiffResult, DiffSource};
use crate::disasm::DisasmRow;
use crate::mode::Mode;
use crate::profile::{FrameStats, RenderMainStats};
use crate::util::format::offset_width;
use crate::view::{
    ascii_grid, command_line, data_panel, diff_panel, disasm_grid, gutter, hex_grid,
    inspector as inspector_view, layout, status, symbol_panel,
};

struct VisibleRows {
    offsets: Vec<u64>,
    rows: Vec<Vec<ByteSlot>>,
}

#[derive(Debug, Clone, Default)]
struct VisibleDiffPage {
    rows: Vec<diff_panel::DiffPanelRow>,
    overlay_spans: Vec<hex_grid::DiffOverlaySpan>,
    main_rows: Vec<Vec<hex_grid::HexGridCell>>,
    main_ascii_rows: Vec<Vec<(ByteSlot, Option<u64>, bool)>>,
    main_row_offsets: Vec<u64>,
}

#[derive(Debug, Clone, Copy)]
struct DiffAlignedCell {
    other_offset: Option<u64>,
}

#[derive(Debug, Default)]
struct DiffAlignment {
    current_cells: HashMap<u64, DiffAlignedCell>,
    other_bytes: HashMap<u64, u8>,
    cells: Vec<AlignedDiffCell>,
}

#[derive(Debug, Clone, Copy)]
struct AlignedDiffCell {
    current: Option<DiffByte>,
    other: Option<DiffByte>,
    anchor_display: Option<u64>,
    visual_display: Option<u64>,
    kind: diff_panel::DiffPanelCellKind,
}

#[derive(Debug, Clone)]
struct WindowDiffSource {
    bytes: Vec<DiffByte>,
    index: usize,
}

impl WindowDiffSource {
    fn new(bytes: Vec<DiffByte>) -> Self {
        Self { bytes, index: 0 }
    }
}

impl DiffSource for WindowDiffSource {
    fn read_next(&mut self, max_bytes: usize) -> crate::error::HxResult<Vec<DiffByte>> {
        if max_bytes == 0 || self.index >= self.bytes.len() {
            return Ok(Vec::new());
        }
        let end = (self.index + max_bytes).min(self.bytes.len());
        let out = self.bytes[self.index..end].to_vec();
        self.index = end;
        Ok(out)
    }
}

const DIFF_RENDER_MAX_SHIFT: usize = 4096;
const DIFF_RENDER_EXTRA_CONTEXT: usize = 64;

fn visible_display_bounds(visible_rows: &VisibleRows) -> Option<(u64, u64)> {
    let start = visible_rows.offsets.first().copied()?;
    let end = visible_rows
        .offsets
        .last()
        .zip(visible_rows.rows.last())
        .map(|(offset, row)| offset.saturating_add(row.len().saturating_sub(1) as u64))?;
    Some((start, end))
}

fn build_diff_alignment(
    current: &[DiffByte],
    other: &[DiffByte],
    result: &DiffResult,
) -> DiffAlignment {
    let mut alignment = DiffAlignment {
        current_cells: HashMap::new(),
        other_bytes: other
            .iter()
            .map(|byte| (byte.stream_offset, byte.byte))
            .collect(),
        cells: Vec::new(),
    };
    let Some(first_current) = current.first() else {
        return alignment;
    };
    let current_end = current
        .last()
        .map(|byte| byte.stream_offset.saturating_add(1))
        .unwrap_or(first_current.stream_offset);
    let mut current_pos = first_current.stream_offset;
    let mut other_pos = other
        .first()
        .map(|byte| byte.stream_offset)
        .unwrap_or_default();

    for hunk in &result.hunks {
        map_equal_alignment(
            &mut alignment,
            current,
            other,
            current_pos,
            hunk.current.logical_start,
            other_pos,
        );
        match hunk.kind {
            DiffHunkKind::OnlyCurrent => {
                map_only_current_alignment(
                    &mut alignment,
                    current,
                    hunk.current.logical_start,
                    hunk.current.logical_len,
                );
            }
            DiffHunkKind::OnlyOther => {
                map_only_other_alignment(
                    &mut alignment,
                    current,
                    other,
                    hunk.current.logical_start,
                    hunk.other.offset,
                    hunk.other.len,
                );
            }
            DiffHunkKind::Replace | DiffHunkKind::Unresolved => {
                map_replace_alignment(
                    &mut alignment,
                    current,
                    other,
                    hunk.current.logical_start,
                    hunk.current.logical_len,
                    hunk.other.offset,
                    hunk.other.len,
                );
            }
        }
        current_pos = hunk
            .current
            .logical_start
            .saturating_add(hunk.current.logical_len);
        other_pos = hunk.other.offset.saturating_add(hunk.other.len);
    }
    map_equal_alignment(
        &mut alignment,
        current,
        other,
        current_pos,
        current_end,
        other_pos,
    );
    alignment
}

fn map_equal_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    other: &[DiffByte],
    current_start: u64,
    current_end: u64,
    other_start: u64,
) {
    if current_start >= current_end {
        return;
    }
    for current_offset in current_start..current_end {
        let delta = current_offset.saturating_sub(current_start);
        alignment.current_cells.insert(
            current_offset,
            DiffAlignedCell {
                other_offset: Some(other_start.saturating_add(delta)),
            },
        );
        let Some(current_byte) = find_diff_byte(current, current_offset) else {
            continue;
        };
        let other_byte = find_diff_byte(other, other_start.saturating_add(delta));
        let kind = match other_byte {
            Some(other_byte) if other_byte.byte == current_byte.byte => {
                diff_panel::DiffPanelCellKind::Equal
            }
            Some(_) => diff_panel::DiffPanelCellKind::Replace,
            None => diff_panel::DiffPanelCellKind::OnlyCurrent,
        };
        alignment.cells.push(AlignedDiffCell {
            current: Some(current_byte),
            other: other_byte,
            anchor_display: current_byte.display_offset,
            visual_display: current_byte.display_offset,
            kind,
        });
    }
}

fn map_only_current_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    current_start: u64,
    len: u64,
) {
    for delta in 0..len {
        let current_offset = current_start.saturating_add(delta);
        alignment
            .current_cells
            .insert(current_offset, DiffAlignedCell { other_offset: None });
        let Some(current_byte) = find_diff_byte(current, current_offset) else {
            continue;
        };
        alignment.cells.push(AlignedDiffCell {
            current: Some(current_byte),
            other: None,
            anchor_display: current_byte.display_offset,
            visual_display: current_byte.display_offset,
            kind: diff_panel::DiffPanelCellKind::OnlyCurrent,
        });
    }
}

fn map_only_other_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    other: &[DiffByte],
    current_anchor: u64,
    other_start: u64,
    len: u64,
) {
    let anchor_display = anchor_display_for_current(current, current_anchor);
    for delta in 0..len {
        let Some(other_byte) = find_diff_byte(other, other_start.saturating_add(delta)) else {
            continue;
        };
        let visual_display = anchor_display.map(|display| display.saturating_add(delta));
        alignment.cells.push(AlignedDiffCell {
            current: None,
            other: Some(other_byte),
            anchor_display,
            visual_display,
            kind: diff_panel::DiffPanelCellKind::OnlyOther,
        });
    }
}

fn map_replace_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    other: &[DiffByte],
    current_start: u64,
    current_len: u64,
    other_start: u64,
    other_len: u64,
) {
    let shared = current_len.min(other_len);
    for delta in 0..shared {
        alignment.current_cells.insert(
            current_start.saturating_add(delta),
            DiffAlignedCell {
                other_offset: Some(other_start.saturating_add(delta)),
            },
        );
        let Some(current_byte) = find_diff_byte(current, current_start.saturating_add(delta))
        else {
            continue;
        };
        let other_byte = find_diff_byte(other, other_start.saturating_add(delta));
        let kind = match other_byte {
            Some(other_byte) if other_byte.byte == current_byte.byte => {
                diff_panel::DiffPanelCellKind::Equal
            }
            Some(_) => diff_panel::DiffPanelCellKind::Replace,
            None => diff_panel::DiffPanelCellKind::OnlyCurrent,
        };
        alignment.cells.push(AlignedDiffCell {
            current: Some(current_byte),
            other: other_byte,
            anchor_display: current_byte.display_offset,
            visual_display: current_byte.display_offset,
            kind,
        });
    }
    if current_len > shared {
        map_only_current_alignment(
            alignment,
            current,
            current_start.saturating_add(shared),
            current_len - shared,
        );
    }
    if other_len > shared {
        map_only_other_alignment(
            alignment,
            current,
            other,
            current_start.saturating_add(shared),
            other_start.saturating_add(shared),
            other_len - shared,
        );
    }
}

fn find_diff_byte(bytes: &[DiffByte], stream_offset: u64) -> Option<DiffByte> {
    let first = bytes.first()?.stream_offset;
    let idx = stream_offset.checked_sub(first)? as usize;
    bytes
        .get(idx)
        .copied()
        .filter(|byte| byte.stream_offset == stream_offset)
}

fn anchor_display_for_current(current: &[DiffByte], current_anchor: u64) -> Option<u64> {
    find_diff_byte(current, current_anchor)
        .and_then(|byte| byte.display_offset)
        .or_else(|| {
            current
                .iter()
                .rev()
                .find(|byte| byte.stream_offset < current_anchor)
                .and_then(|byte| byte.display_offset)
                .map(|display| display.saturating_add(1))
        })
}

fn aligned_cell_display_offset(cell: &AlignedDiffCell) -> Option<u64> {
    cell.current
        .and_then(|byte| byte.display_offset)
        .or(cell.visual_display)
        .or(cell.anchor_display)
}

fn diff_overlay_kind_for_panel_kind(
    kind: diff_panel::DiffPanelCellKind,
) -> Option<hex_grid::DiffOverlayKind> {
    match kind {
        diff_panel::DiffPanelCellKind::Replace => Some(hex_grid::DiffOverlayKind::Replace),
        diff_panel::DiffPanelCellKind::OnlyCurrent => Some(hex_grid::DiffOverlayKind::OnlyCurrent),
        diff_panel::DiffPanelCellKind::OnlyOther => Some(hex_grid::DiffOverlayKind::OnlyOther),
        diff_panel::DiffPanelCellKind::Equal | diff_panel::DiffPanelCellKind::Gap => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffCellHit {
    pub side: crate::input::mouse::DiffCellSide,
    pub visual_offset: u64,
    pub current_display_offset: Option<u64>,
    pub other_offset: Option<u64>,
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
        self.render_side_panel(frame, columns);

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

    fn build_hex_main_lines(&mut self, visible_rows: &VisibleRows) -> MainLines {
        if self.document.is_empty() {
            return MainLines {
                gutter: vec![Line::raw("No data")],
                pane: MainPaneLines::Hex {
                    hex: vec![Line::raw("No content")],
                    ascii: vec![Line::raw("")],
                },
            };
        }

        let selection = self.selection_range();
        let inspector_highlight = self.inspector_highlight_range();
        let search_matches = self.visible_search_matches(visible_rows);
        let diff_page = self.visible_diff_page(visible_rows).unwrap_or_else(|err| {
            self.report_render_error(format!("diff render failed: {err}"));
            VisibleDiffPage::default()
        });
        let projected = !diff_page.main_rows.is_empty();
        let gutter_offsets = if projected {
            &diff_page.main_row_offsets
        } else {
            &visible_rows.offsets
        };
        let gutter_lines = gutter::build(
            gutter_offsets,
            offset_width(self.document.len()),
            &self.palette,
        );
        let overlays = hex_grid::HexGridOverlays {
            diff_spans: diff_page.overlay_spans,
            selection,
            inspector_highlight,
            search_matches,
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
                    selection,
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
                    selection,
                ),
            )
        };
        MainLines {
            gutter: gutter_lines,
            pane: MainPaneLines::Hex { hex, ascii },
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

    fn visible_diff_page(
        &mut self,
        visible_rows: &VisibleRows,
    ) -> crate::error::HxResult<VisibleDiffPage> {
        if !self.diff_projection_active() || visible_rows.offsets.is_empty() {
            return Ok(VisibleDiffPage::default());
        }

        let alignment = self.diff_alignment_for_visible(visible_rows)?;
        if let Some(alignment) = alignment
            .as_ref()
            .filter(|alignment| !alignment.cells.is_empty())
        {
            return Ok(self.projected_visible_diff_page(visible_rows, alignment));
        }
        let mut page = VisibleDiffPage::default();
        for (row_idx, row) in visible_rows.rows.iter().enumerate() {
            let row_offset = visible_rows
                .offsets
                .get(row_idx)
                .copied()
                .unwrap_or_default();
            let mut cells = Vec::with_capacity(row.len());
            for (col_idx, slot) in row.iter().enumerate() {
                let display_offset = row_offset + col_idx as u64;
                let (other_byte, kind) =
                    self.diff_cell_for_slot(display_offset, *slot, alignment.as_ref())?;
                if matches!(
                    kind,
                    diff_panel::DiffPanelCellKind::Replace
                        | diff_panel::DiffPanelCellKind::OnlyCurrent
                        | diff_panel::DiffPanelCellKind::OnlyOther
                ) {
                    let overlay_kind = match kind {
                        diff_panel::DiffPanelCellKind::Replace => {
                            hex_grid::DiffOverlayKind::Replace
                        }
                        diff_panel::DiffPanelCellKind::OnlyCurrent => {
                            hex_grid::DiffOverlayKind::OnlyCurrent
                        }
                        diff_panel::DiffPanelCellKind::OnlyOther => {
                            hex_grid::DiffOverlayKind::OnlyOther
                        }
                        diff_panel::DiffPanelCellKind::Equal
                        | diff_panel::DiffPanelCellKind::Gap => {
                            unreachable!("only mismatch kinds are overlaid")
                        }
                    };
                    page.overlay_spans.push(hex_grid::DiffOverlaySpan {
                        start: display_offset,
                        end: display_offset,
                        kind: overlay_kind,
                        active: false,
                    });
                }
                let other_offset = if other_byte.is_some() {
                    self.document
                        .logical_offset_for_display_offset(display_offset)
                        .or(Some(display_offset))
                } else {
                    None
                };
                let current_display_offset =
                    matches!(slot, ByteSlot::Present(_)).then_some(display_offset);
                cells.push(diff_panel::DiffPanelCell {
                    other_byte,
                    kind,
                    other_offset,
                    current_display_offset,
                    visual_display_offset: Some(display_offset),
                    active: display_offset == self.cursor,
                });
            }
            page.rows.push(diff_panel::DiffPanelRow {
                display_offset: row_offset,
                cells,
            });
        }
        Ok(page)
    }

    fn projected_visible_diff_page(
        &self,
        visible_rows: &VisibleRows,
        alignment: &DiffAlignment,
    ) -> VisibleDiffPage {
        let Some(visible_start) = visible_rows.offsets.first().copied() else {
            return VisibleDiffPage::default();
        };
        let start_idx = alignment
            .cells
            .iter()
            .position(|cell| {
                cell.anchor_display
                    .map(|display| display >= visible_start)
                    .unwrap_or(false)
            })
            .unwrap_or(alignment.cells.len());
        let mut idx = start_idx;
        let mut page = VisibleDiffPage::default();
        let bytes_per_line = self.config.bytes_per_line;

        for row_idx in 0..visible_rows.rows.len() {
            let fallback_offset = visible_rows
                .offsets
                .get(row_idx)
                .copied()
                .unwrap_or(visible_start);
            let row_offset = alignment
                .cells
                .get(idx)
                .and_then(aligned_cell_display_offset)
                .unwrap_or(fallback_offset);
            page.main_row_offsets.push(row_offset);

            let mut panel_cells = Vec::with_capacity(bytes_per_line);
            let mut main_cells = Vec::with_capacity(bytes_per_line);
            let mut ascii_cells = Vec::with_capacity(bytes_per_line);
            for _ in 0..bytes_per_line {
                let Some(cell) = alignment.cells.get(idx).copied() else {
                    panel_cells.push(diff_panel::DiffPanelCell {
                        other_byte: None,
                        kind: diff_panel::DiffPanelCellKind::Gap,
                        other_offset: None,
                        current_display_offset: None,
                        visual_display_offset: None,
                        active: false,
                    });
                    main_cells.push(hex_grid::HexGridCell {
                        slot: ByteSlot::Empty,
                        display_offset: None,
                        diff: None,
                        visual_offset: None,
                        other_offset: None,
                    });
                    ascii_cells.push((ByteSlot::Empty, None, false));
                    continue;
                };
                idx += 1;
                let slot = cell
                    .current
                    .map(|byte| ByteSlot::Present(byte.byte))
                    .unwrap_or(ByteSlot::Empty);
                let display_offset = cell.current.and_then(|byte| byte.display_offset);
                let visual_offset = cell
                    .current
                    .and_then(|byte| byte.display_offset)
                    .or(cell.visual_display)
                    .or(cell.anchor_display);
                let diff = diff_overlay_kind_for_panel_kind(cell.kind);
                panel_cells.push(diff_panel::DiffPanelCell {
                    other_byte: cell.other.map(|byte| byte.byte),
                    kind: cell.kind,
                    other_offset: cell.other.map(|byte| byte.stream_offset),
                    current_display_offset: display_offset,
                    visual_display_offset: visual_offset,
                    active: self.diff_aligned_cell_is_active(cell),
                });
                main_cells.push(hex_grid::HexGridCell {
                    slot,
                    display_offset,
                    diff,
                    visual_offset,
                    other_offset: cell.other.map(|byte| byte.stream_offset),
                });
                ascii_cells.push((
                    slot,
                    display_offset,
                    cell.kind == diff_panel::DiffPanelCellKind::OnlyOther,
                ));
            }
            page.rows.push(diff_panel::DiffPanelRow {
                display_offset: row_offset,
                cells: panel_cells,
            });
            page.main_rows.push(main_cells);
            page.main_ascii_rows.push(ascii_cells);
        }

        page
    }

    fn diff_alignment_for_visible(
        &mut self,
        visible_rows: &VisibleRows,
    ) -> crate::error::HxResult<Option<DiffAlignment>> {
        if !self.diff_projection_active() {
            return Ok(None);
        }
        let Some(state) = self.diff_state() else {
            return Ok(None);
        };
        let options = state.options;
        if options.max_shift == 0 || self.document.is_empty() {
            return Ok(None);
        }
        let Some((visible_start, visible_end)) = visible_display_bounds(visible_rows) else {
            return Ok(None);
        };

        let render_shift = options.max_shift.min(DIFF_RENDER_MAX_SHIFT);
        if render_shift == 0 {
            return Ok(None);
        }
        let context = render_shift
            .saturating_add(options.anchor_len)
            .saturating_add(options.verify_len)
            .saturating_add(DIFF_RENDER_EXTRA_CONTEXT);
        let display_start = visible_start.saturating_sub(context as u64);
        let display_end = visible_end
            .saturating_add(context as u64)
            .min(self.document.len().saturating_sub(1));
        let current = self.collect_diff_window_bytes(display_start, display_end)?;
        if current.is_empty() {
            return Ok(Some(DiffAlignment::default()));
        }

        let current_first = current
            .first()
            .map(|byte| byte.stream_offset)
            .unwrap_or_default();
        let current_last = current
            .last()
            .map(|byte| byte.stream_offset)
            .unwrap_or(current_first);
        let other_start = current_first.saturating_sub(render_shift as u64);
        let other_end = current_last
            .saturating_add(context as u64)
            .saturating_add(render_shift as u64);
        let other = self.collect_other_diff_window_bytes(other_start, other_end)?;
        if other.is_empty() {
            let mut alignment = DiffAlignment::default();
            for byte in &current {
                alignment
                    .current_cells
                    .insert(byte.stream_offset, DiffAlignedCell { other_offset: None });
            }
            return Ok(Some(alignment));
        }

        let mut local_options = options;
        local_options.max_shift = render_shift.saturating_mul(2).max(1);
        local_options.hunk_cap = options.hunk_cap.max(visible_rows.rows.len().max(1) * 4);
        let result = diff_sources(
            WindowDiffSource::new(current.clone()),
            WindowDiffSource::new(other.clone()),
            local_options,
        )?;
        Ok(Some(build_diff_alignment(&current, &other, &result)))
    }

    fn collect_diff_window_bytes(
        &mut self,
        display_start: u64,
        display_end: u64,
    ) -> crate::error::HxResult<Vec<DiffByte>> {
        if self.document.is_empty() || display_start > display_end {
            return Ok(Vec::new());
        }
        let end = display_end.min(self.document.len().saturating_sub(1));
        let mut out = Vec::with_capacity((end - display_start + 1).min(8192) as usize);
        for display_offset in display_start..=end {
            let ByteSlot::Present(byte) = self.document.byte_at(display_offset)? else {
                continue;
            };
            if let Some(logical_offset) = self
                .document
                .logical_offset_for_display_offset(display_offset)
            {
                out.push(DiffByte {
                    stream_offset: logical_offset,
                    display_offset: Some(display_offset),
                    byte,
                });
            }
        }
        Ok(out)
    }

    fn collect_other_diff_window_bytes(
        &mut self,
        other_start: u64,
        other_end: u64,
    ) -> crate::error::HxResult<Vec<DiffByte>> {
        let Some(state) = self.diff_state_mut() else {
            return Ok(Vec::new());
        };
        if other_start > other_end || other_start >= state.other_len {
            return Ok(Vec::new());
        }
        let end = other_end.min(state.other_len.saturating_sub(1));
        let len = (end - other_start + 1) as usize;
        let raw = state.other_view.read_range(other_start, len)?;
        Ok(raw
            .into_iter()
            .enumerate()
            .map(|(idx, byte)| DiffByte {
                stream_offset: other_start + idx as u64,
                display_offset: None,
                byte,
            })
            .collect())
    }

    fn diff_cell_for_slot(
        &mut self,
        display_offset: u64,
        slot: ByteSlot,
        alignment: Option<&DiffAlignment>,
    ) -> crate::error::HxResult<(Option<u8>, diff_panel::DiffPanelCellKind)> {
        let current = match slot {
            ByteSlot::Present(current) => current,
            ByteSlot::Empty => {
                let other = self.read_diff_other_byte(display_offset)?;
                let kind = if other.is_some() {
                    diff_panel::DiffPanelCellKind::OnlyOther
                } else {
                    diff_panel::DiffPanelCellKind::Gap
                };
                return Ok((other, kind));
            }
            ByteSlot::Deleted => return Ok((None, diff_panel::DiffPanelCellKind::Gap)),
        };
        let Some(logical_offset) = self
            .document
            .logical_offset_for_display_offset(display_offset)
        else {
            return Ok((None, diff_panel::DiffPanelCellKind::Gap));
        };
        if let Some(alignment) = alignment {
            if let Some(cell) = alignment.current_cells.get(&logical_offset) {
                let Some(other_offset) = cell.other_offset else {
                    return Ok((None, diff_panel::DiffPanelCellKind::OnlyCurrent));
                };
                let Some(&other_byte) = alignment.other_bytes.get(&other_offset) else {
                    return Ok((None, diff_panel::DiffPanelCellKind::OnlyCurrent));
                };
                let kind = if other_byte == current {
                    diff_panel::DiffPanelCellKind::Equal
                } else {
                    diff_panel::DiffPanelCellKind::Replace
                };
                return Ok((Some(other_byte), kind));
            }
        }
        let other = self.read_diff_other_byte(logical_offset)?;
        let kind = match other {
            Some(byte) if byte == current => diff_panel::DiffPanelCellKind::Equal,
            Some(_) => diff_panel::DiffPanelCellKind::Replace,
            None => diff_panel::DiffPanelCellKind::OnlyCurrent,
        };
        Ok((other, kind))
    }

    fn diff_aligned_cell_is_active(&self, cell: AlignedDiffCell) -> bool {
        match cell.kind {
            diff_panel::DiffPanelCellKind::OnlyOther => {
                let Some(state) = self.diff_state() else {
                    return false;
                };
                let Some(other_offset) = cell.other.map(|byte| byte.stream_offset) else {
                    return false;
                };
                state.selected_other_offset == Some(other_offset)
                    && state
                        .selected_other_anchor_display
                        .is_some_and(|anchor| anchor == self.cursor)
            }
            _ => cell
                .current
                .and_then(|byte| byte.display_offset)
                .is_some_and(|display| display == self.cursor),
        }
    }

    pub(crate) fn visible_diff_cell_hit(
        &mut self,
        visible_row: usize,
        col: usize,
        side: crate::input::mouse::DiffCellSide,
    ) -> Option<DiffCellHit> {
        if !self.diff_projection_active() || col >= self.config.bytes_per_line {
            return None;
        }
        let visible_rows = self.collect_visible_rows(self.view_rows);
        let page = self.visible_diff_page(&visible_rows).ok()?;
        let row = page.rows.get(visible_row)?;
        let cell = row.cells.get(col)?;
        if matches!(cell.kind, diff_panel::DiffPanelCellKind::Gap) {
            return None;
        }
        match side {
            crate::input::mouse::DiffCellSide::Current => {
                let visual_offset = cell
                    .visual_display_offset
                    .or(cell.current_display_offset)
                    .or_else(|| page.main_rows.get(visible_row)?.get(col)?.visual_offset)?;
                Some(DiffCellHit {
                    side,
                    visual_offset,
                    current_display_offset: cell.current_display_offset,
                    other_offset: cell.other_offset,
                })
            }
            crate::input::mouse::DiffCellSide::Other => {
                let other_offset = cell.other_offset?;
                let visual_offset = cell
                    .visual_display_offset
                    .or(cell.current_display_offset)
                    .or_else(|| page.main_rows.get(visible_row)?.get(col)?.visual_offset)
                    .unwrap_or(row.display_offset.saturating_add(col as u64));
                Some(DiffCellHit {
                    side,
                    visual_offset,
                    current_display_offset: cell.current_display_offset,
                    other_offset: Some(other_offset),
                })
            }
        }
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

    fn render_side_panel(&mut self, frame: &mut ratatui::Frame<'_>, columns: layout::MainColumns) {
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
                    frame.render_widget(
                        Paragraph::new(error.clone()).wrap(Wrap { trim: false }),
                        side_panel_area,
                    );
                } else {
                    frame.render_widget(
                        Paragraph::new(self.inspector_empty_panel_message())
                            .wrap(Wrap { trim: false }),
                        side_panel_area,
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
        let visible_rows = self.collect_visible_rows(area.height as usize);
        let page = self.visible_diff_page(&visible_rows).unwrap_or_else(|err| {
            self.report_render_error(format!("diff panel render failed: {err}"));
            VisibleDiffPage::default()
        });
        let lines = diff_panel::build_lines(
            &page.rows,
            offset_width(self.document.len()),
            self.config.bytes_per_line,
            &self.palette,
        );
        let visible_height = area.height as usize;
        let visible_end = visible_height.min(lines.len());
        frame.render_widget(Paragraph::new(lines[..visible_end].to_vec()), area);
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

        if visible_row < self.side_panel_visible_rows() {
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

    #[test]
    fn visible_diff_page_marks_equal_replace_and_missing_sides() {
        let dir = tempdir().unwrap();
        let other = dir.path().join("other.bin");
        fs::write(&other, b"axcde").unwrap();
        let mut app = app_with_bytes(b"abcd");
        app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
            path: other,
            max_shift: None,
        }))
        .unwrap();

        let visible_rows = app.collect_visible_rows(1);
        let page = app.visible_diff_page(&visible_rows).unwrap();
        let row = &page.rows[0];
        assert_eq!(
            row.cells[0].kind,
            crate::view::diff_panel::DiffPanelCellKind::Equal
        );
        assert_eq!(
            row.cells[1].kind,
            crate::view::diff_panel::DiffPanelCellKind::Replace
        );
        assert_eq!(
            row.cells[4].kind,
            crate::view::diff_panel::DiffPanelCellKind::OnlyOther
        );
        assert_eq!(
            page.main_rows[0][4].diff,
            Some(crate::view::hex_grid::DiffOverlayKind::OnlyOther)
        );

        let dir = tempdir().unwrap();
        let other = dir.path().join("other-short.bin");
        fs::write(&other, b"abc").unwrap();
        let mut app = app_with_bytes(b"abcd");
        app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
            path: other,
            max_shift: None,
        }))
        .unwrap();
        let visible_rows = app.collect_visible_rows(1);
        let page = app.visible_diff_page(&visible_rows).unwrap();
        assert_eq!(
            page.rows[0].cells[3].kind,
            crate::view::diff_panel::DiffPanelCellKind::OnlyCurrent
        );
    }

    #[test]
    fn visible_diff_page_realizes_after_current_side_insert() {
        let dir = tempdir().unwrap();
        let other = dir.path().join("other.bin");
        fs::write(&other, b"abcdefghijklmnopqrstuvwxyz0123456789").unwrap();
        let mut app = app_with_bytes(b"abXcdefghijklmnopqrstuvwxyz0123456789");
        app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
            path: other,
            max_shift: None,
        }))
        .unwrap();

        let visible_rows = app.collect_visible_rows(1);
        let page = app.visible_diff_page(&visible_rows).unwrap();
        let row = &page.rows[0];
        assert_eq!(
            row.cells[0].kind,
            crate::view::diff_panel::DiffPanelCellKind::Equal
        );
        assert_eq!(
            row.cells[1].kind,
            crate::view::diff_panel::DiffPanelCellKind::Equal
        );
        assert_eq!(
            row.cells[2].kind,
            crate::view::diff_panel::DiffPanelCellKind::OnlyCurrent
        );
        assert_eq!(row.cells[2].other_byte, None);
        assert_eq!(
            row.cells[3].kind,
            crate::view::diff_panel::DiffPanelCellKind::Equal
        );
        assert_eq!(row.cells[3].other_byte, Some(b'c'));
        assert_eq!(
            row.cells[4].kind,
            crate::view::diff_panel::DiffPanelCellKind::Equal
        );
        assert_eq!(row.cells[4].other_byte, Some(b'd'));
    }

    #[test]
    fn visible_diff_page_realizes_zip_like_mid_file_insert() {
        let dir = tempdir().unwrap();
        let other = dir.path().join("other.bin");
        let base = (0..0x140)
            .map(|idx| (idx as u8).wrapping_mul(37).wrapping_add(11))
            .collect::<Vec<_>>();
        fs::write(&other, &base).unwrap();
        let mut current = base.clone();
        current.insert(0xba, 0xab);
        let mut app = app_with_bytes(&current);
        app.viewport_top = 0xb0;
        app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
            path: other,
            max_shift: None,
        }))
        .unwrap();

        let visible_rows = app.collect_visible_rows(1);
        let page = app.visible_diff_page(&visible_rows).unwrap();
        let row = &page.rows[0];
        assert_eq!(
            row.cells[0x0a].kind,
            crate::view::diff_panel::DiffPanelCellKind::OnlyCurrent
        );
        assert_eq!(row.cells[0x0a].other_byte, None);
        assert_eq!(
            row.cells[0x0b].kind,
            crate::view::diff_panel::DiffPanelCellKind::Equal
        );
        assert_eq!(row.cells[0x0b].other_byte, Some(base[0xba]));
    }

    #[test]
    fn visible_diff_page_shows_other_side_insert_as_placeholder_on_left() {
        let dir = tempdir().unwrap();
        let other = dir.path().join("other.bin");
        let base = (0..0x140)
            .map(|idx| (idx as u8).wrapping_mul(37).wrapping_add(11))
            .collect::<Vec<_>>();
        let mut other_bytes = base.clone();
        other_bytes.insert(0xba, 0xab);
        fs::write(&other, &other_bytes).unwrap();
        let mut app = app_with_bytes(&base);
        app.viewport_top = 0xb0;
        app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
            path: other,
            max_shift: None,
        }))
        .unwrap();

        let visible_rows = app.collect_visible_rows(1);
        let page = app.visible_diff_page(&visible_rows).unwrap();
        let row = &page.rows[0];
        assert_eq!(
            row.cells[0x0a].kind,
            crate::view::diff_panel::DiffPanelCellKind::OnlyOther
        );
        assert_eq!(row.cells[0x0a].other_byte, Some(0xab));
        assert_eq!(
            page.main_rows[0][0x0a].slot,
            crate::core::document::ByteSlot::Empty
        );
        assert_eq!(
            page.main_rows[0][0x0a].diff,
            Some(crate::view::hex_grid::DiffOverlayKind::OnlyOther)
        );
        assert_eq!(page.main_rows[0][0x0a].display_offset, None);
        assert_eq!(page.main_rows[0][0x0a].visual_offset, Some(0xba));
        assert_eq!(row.cells[0x0a].current_display_offset, None);
        assert_eq!(row.cells[0x0a].visual_display_offset, Some(0xba));
        assert_eq!(row.cells[0x0a].other_offset, Some(0xba));
        assert_eq!(
            row.cells[0x0b].kind,
            crate::view::diff_panel::DiffPanelCellKind::Equal
        );
        assert_eq!(row.cells[0x0b].other_byte, Some(base[0xba]));
        assert_eq!(page.main_rows[0][0x0b].display_offset, Some(0xba));
    }

    #[test]
    fn diff_overlay_is_removed_when_diff_side_panel_is_not_active() {
        let dir = tempdir().unwrap();
        let other = dir.path().join("other.bin");
        fs::write(&other, b"abXc").unwrap();
        let mut app = app_with_bytes(b"abc");
        app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
            path: other,
            max_shift: None,
        }))
        .unwrap();
        let visible_rows = app.collect_visible_rows(1);
        assert!(!app
            .visible_diff_page(&visible_rows)
            .unwrap()
            .main_rows
            .is_empty());

        app.show_side_panel = false;
        app.active_side_panel = crate::app::SidePanelKind::Inspector;
        let hidden_page = app.visible_diff_page(&visible_rows).unwrap();
        assert!(hidden_page.rows.is_empty());
        assert!(hidden_page.main_rows.is_empty());
        assert!(hidden_page.overlay_spans.is_empty());
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
