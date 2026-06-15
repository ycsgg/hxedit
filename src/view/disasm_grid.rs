use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::disasm::text::{
    for_each_instruction_text_token, looks_like_immediate, looks_like_register,
    InstructionTextTokenKind,
};
use crate::disasm::{DisasmFunctionBoundary, DisasmRow, DisasmRowKind};
use crate::view::palette::Palette;

pub const JUMP_RAIL_WIDTH: usize = 25;
const JUMP_RAIL_LANE_WIDTH: usize = 3;
pub const JUMP_RAIL_MIN_TEXT_WIDTH: usize = 48;
const JUMP_RAIL_MAX_LANES: usize = 8;
const JUMP_RAIL_INNER_OFFSET: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisasmVisualRow {
    FunctionStart(usize),
    Row(usize),
    FunctionEnd(usize),
}

pub struct DisasmDisplayLines {
    pub gutter: Vec<Line<'static>>,
    pub bytes: Vec<Line<'static>>,
    pub text: Vec<Line<'static>>,
    pub row_sources: Vec<Option<usize>>,
    pub hit_sources: Vec<Option<DisasmHitSource>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisasmHitSource {
    pub row_index: usize,
    pub row_start_only: bool,
}

pub fn build_display(
    rows: &[DisasmRow],
    gutter_width: usize,
    cursor: u64,
    editing: Option<(u64, &str)>,
    text_width: usize,
    palette: &Palette,
) -> DisasmDisplayLines {
    let mut display = DisasmDisplayLines {
        gutter: Vec::with_capacity(rows.len()),
        bytes: Vec::with_capacity(rows.len()),
        text: Vec::with_capacity(rows.len()),
        row_sources: Vec::with_capacity(rows.len()),
        hit_sources: Vec::with_capacity(rows.len()),
    };

    for visual in visual_rows(rows) {
        match visual {
            DisasmVisualRow::FunctionStart(index) => {
                display.gutter.push(Line::raw(""));
                display.bytes.push(Line::raw(""));
                display
                    .text
                    .push(function_boundary_text(&rows[index], true, palette));
                display.row_sources.push(None);
                display.hit_sources.push(Some(DisasmHitSource {
                    row_index: index,
                    row_start_only: true,
                }));
            }
            DisasmVisualRow::FunctionEnd(index) => {
                display.gutter.push(Line::raw(""));
                display.bytes.push(Line::raw(""));
                display
                    .text
                    .push(function_boundary_text(&rows[index], false, palette));
                display.row_sources.push(None);
                display.hit_sources.push(Some(DisasmHitSource {
                    row_index: index,
                    row_start_only: true,
                }));
            }
            DisasmVisualRow::Row(index) => {
                let row = &rows[index];
                let line = build_text_row(row, cursor, editing, palette);
                let continuation_prefix = text_continuation_prefix(row, cursor, palette);
                let wrapped =
                    wrap_line_with_continuation_prefix(line, continuation_prefix, text_width);
                for (line_index, line) in wrapped.into_iter().enumerate() {
                    if line_index == 0 {
                        display
                            .gutter
                            .push(build_gutter_row(row, gutter_width, cursor, palette));
                        display.bytes.push(build_bytes_row(row, cursor, palette));
                    } else {
                        display.gutter.push(Line::raw(""));
                        display.bytes.push(Line::raw(""));
                    }
                    display.text.push(line);
                    display.row_sources.push(Some(index));
                    display.hit_sources.push(Some(DisasmHitSource {
                        row_index: index,
                        row_start_only: line_index > 0,
                    }));
                }
            }
        }
    }

    display
}

pub fn build_gutter(
    rows: &[DisasmRow],
    width: usize,
    cursor: u64,
    palette: &Palette,
) -> Vec<Line<'static>> {
    visual_rows(rows)
        .into_iter()
        .map(|visual| match visual {
            DisasmVisualRow::FunctionStart(_) | DisasmVisualRow::FunctionEnd(_) => Line::raw(""),
            DisasmVisualRow::Row(index) => build_gutter_row(&rows[index], width, cursor, palette),
        })
        .collect()
}

pub fn build_bytes(rows: &[DisasmRow], cursor: u64, palette: &Palette) -> Vec<Line<'static>> {
    visual_rows(rows)
        .into_iter()
        .map(|visual| match visual {
            DisasmVisualRow::FunctionStart(_) | DisasmVisualRow::FunctionEnd(_) => Line::raw(""),
            DisasmVisualRow::Row(index) => build_bytes_row(&rows[index], cursor, palette),
        })
        .collect()
}

pub fn build_text(
    rows: &[DisasmRow],
    cursor: u64,
    editing: Option<(u64, &str)>,
    palette: &Palette,
) -> Vec<Line<'static>> {
    visual_rows(rows)
        .into_iter()
        .map(|visual| match visual {
            DisasmVisualRow::FunctionStart(index) => {
                function_boundary_text(&rows[index], true, palette)
            }
            DisasmVisualRow::FunctionEnd(index) => {
                function_boundary_text(&rows[index], false, palette)
            }
            DisasmVisualRow::Row(index) => build_text_row(&rows[index], cursor, editing, palette),
        })
        .collect()
}

pub fn visual_row_source_indices(rows: &[DisasmRow]) -> Vec<Option<usize>> {
    visual_rows(rows)
        .into_iter()
        .map(|visual| match visual {
            DisasmVisualRow::FunctionStart(_) | DisasmVisualRow::FunctionEnd(_) => None,
            DisasmVisualRow::Row(index) => Some(index),
        })
        .collect()
}

pub fn build_jump_rail(
    rows: &[DisasmRow],
    row_sources: &[Option<usize>],
    text: &[Line<'_>],
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let lanes = ((JUMP_RAIL_WIDTH - JUMP_RAIL_INNER_OFFSET) / JUMP_RAIL_LANE_WIDTH + 1)
        .clamp(1, JUMP_RAIL_MAX_LANES);
    let text_widths = text.iter().map(line_width).collect::<Vec<_>>();
    let mut cells = vec![vec![JumpRailCell::default(); width]; row_sources.len()];
    let mut first_display_row_by_source = vec![None; rows.len()];
    for (display_row, source) in row_sources.iter().enumerate() {
        if let Some(source) = source {
            first_display_row_by_source[*source].get_or_insert(display_row);
        }
    }

    let mut visible_edges = Vec::new();
    for (source_index, row) in rows.iter().enumerate() {
        let Some(source_y) = first_display_row_by_source
            .get(source_index)
            .copied()
            .flatten()
        else {
            continue;
        };
        let Some(target) = row.direct_target.as_ref() else {
            continue;
        };
        let Some(target_index) = row_index_containing_virtual_address(rows, target.virtual_address)
        else {
            let marker = offscreen_marker(row.virtual_address, target.virtual_address);
            set_rail_marker(
                &mut cells,
                source_y,
                rail_start_col(
                    text_widths.get(source_y).copied().unwrap_or_default(),
                    width,
                ),
                marker,
            );
            continue;
        };
        let Some(target_y) = first_display_row_by_source
            .get(target_index)
            .copied()
            .flatten()
        else {
            continue;
        };
        if source_y == target_y {
            set_rail_marker(
                &mut cells,
                source_y,
                rail_start_col(
                    text_widths.get(source_y).copied().unwrap_or_default(),
                    width,
                ),
                '↺',
            );
            continue;
        }
        visible_edges.push(JumpRailEdge { source_y, target_y });
    }

    visible_edges.sort_by_key(|edge| edge.span());
    let mut lane_ranges: Vec<Vec<(usize, usize)>> = vec![Vec::new(); lanes];
    for edge in visible_edges {
        let Some(lane) = allocate_jump_lane(&lane_ranges, &edge) else {
            set_rail_marker(&mut cells, edge.source_y, width.saturating_sub(1), '⋮');
            continue;
        };
        if draw_jump_edge(&mut cells, &text_widths, edge, lane, width) {
            lane_ranges[lane].push(edge.range());
        } else {
            set_rail_marker(&mut cells, edge.source_y, width.saturating_sub(1), '⋮');
        }
    }

    cells
        .into_iter()
        .map(|chars| {
            Line::from(vec![Span::styled(
                chars
                    .into_iter()
                    .map(JumpRailCell::display_char)
                    .collect::<String>(),
                palette.disasm_symbol,
            )])
        })
        .collect()
}

pub fn merge_jump_rail(
    text: Vec<Line<'static>>,
    jump_rail: &[Line<'_>],
    palette: &Palette,
) -> Vec<Line<'static>> {
    text.into_iter()
        .enumerate()
        .map(|(index, line)| {
            let Some(rail) = jump_rail.get(index) else {
                return line;
            };
            merge_jump_rail_line(line, rail, palette)
        })
        .collect()
}

fn visual_rows(rows: &[DisasmRow]) -> Vec<DisasmVisualRow> {
    let mut visual = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        if row_function_starts(row) {
            visual.push(DisasmVisualRow::FunctionStart(index));
        }
        visual.push(DisasmVisualRow::Row(index));
        if row_function_ends(row) {
            visual.push(DisasmVisualRow::FunctionEnd(index));
        }
    }
    visual
}

fn build_gutter_row(
    row: &DisasmRow,
    width: usize,
    cursor: u64,
    palette: &Palette,
) -> Line<'static> {
    let label_style = if row_contains_cursor(row, cursor) {
        palette.cursor.patch(palette.disasm_label)
    } else {
        palette.disasm_label
    };
    Line::from(vec![Span::styled(
        truncate_label(&row.label(), width),
        label_style,
    )])
}

fn build_bytes_row(row: &DisasmRow, cursor: u64, palette: &Palette) -> Line<'static> {
    let style = match row.kind {
        DisasmRowKind::Instruction => palette.disasm_bytes,
        DisasmRowKind::Data => palette.disasm_data,
        DisasmRowKind::Invalid => palette.warning,
    };
    if row.bytes.is_empty() {
        Line::from(vec![Span::styled("--", palette.separator)])
    } else {
        let mut spans = Vec::with_capacity(row.bytes.len() * 2);
        for (idx, byte) in row.bytes.iter().enumerate() {
            if idx > 0 {
                spans.push(Span::raw(" "));
            }
            let byte_style = if row.offset + idx as u64 == cursor {
                palette.cursor.patch(style)
            } else {
                style
            };
            spans.push(Span::styled(format!("{byte:02x}"), byte_style));
        }
        Line::from(spans)
    }
}

fn build_text_row(
    row: &DisasmRow,
    cursor: u64,
    editing: Option<(u64, &str)>,
    palette: &Palette,
) -> Line<'static> {
    match row.kind {
        DisasmRowKind::Instruction => {
            if editing.is_some_and(|(row_offset, _)| row_offset == row.offset) {
                build_edit_text(
                    row,
                    editing.map(|(_, buffer)| buffer).unwrap_or_default(),
                    cursor,
                    palette,
                )
            } else {
                build_instruction_text(row, cursor, palette)
            }
        }
        DisasmRowKind::Data => build_plain_text_row(row, cursor, palette.disasm_data, palette),
        DisasmRowKind::Invalid => build_plain_text_row(row, cursor, palette.warning, palette),
    }
}

fn build_plain_text_row(
    row: &DisasmRow,
    cursor: u64,
    base_style: Style,
    palette: &Palette,
) -> Line<'static> {
    let active_row = row_contains_cursor(row, cursor);
    let style = if active_row {
        palette.cursor.patch(base_style)
    } else {
        base_style
    };
    let mut spans = function_rail_spans(row, active_row, palette);
    spans.push(Span::styled(row.text.clone(), style));
    append_row_suffix(&mut spans, row, active_row, palette);
    Line::from(spans)
}

fn text_continuation_prefix(row: &DisasmRow, cursor: u64, palette: &Palette) -> Vec<Span<'static>> {
    function_rail_spans(row, row_contains_cursor(row, cursor), palette)
}

fn wrap_line_with_continuation_prefix(
    line: Line<'static>,
    continuation_prefix: Vec<Span<'static>>,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    if line_width(&line) <= width {
        return vec![line];
    }

    let prefix_width = spans_width(&continuation_prefix);
    if !continuation_prefix.is_empty() && prefix_width >= width {
        return vec![line];
    }

    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0;

    for span in line.spans {
        let style = span.style;
        for ch in span.content.chars() {
            if current_width >= width {
                lines.push(Line::from(current));
                current = continuation_prefix.clone();
                current_width = prefix_width;
            }
            push_styled_char(&mut current, ch, style);
            current_width += 1;
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(Line::from(current));
    }
    lines
}

fn push_styled_char(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push(ch);
            return;
        }
    }
    spans.push(Span::styled(ch.to_string(), style));
}

fn line_width(line: &Line<'_>) -> usize {
    spans_width(&line.spans)
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

const RAIL_UP: u8 = 0b0001;
const RAIL_DOWN: u8 = 0b0010;
const RAIL_LEFT: u8 = 0b0100;
const RAIL_RIGHT: u8 = 0b1000;

#[derive(Debug, Clone, Copy, Default)]
struct JumpRailCell {
    dirs: u8,
    arrow_left: bool,
    marker: Option<char>,
}

impl JumpRailCell {
    fn add_dirs(&mut self, dirs: u8) {
        self.dirs |= dirs;
    }

    fn display_char(self) -> char {
        if let Some(marker) = self.marker {
            return marker;
        }
        if self.arrow_left {
            return '◀';
        }
        match self.dirs {
            0 => ' ',
            dirs if dirs == (RAIL_UP | RAIL_DOWN) => '│',
            dirs if dirs == (RAIL_LEFT | RAIL_RIGHT) => '─',
            dirs if dirs == (RAIL_DOWN | RAIL_RIGHT) => '┌',
            dirs if dirs == (RAIL_DOWN | RAIL_LEFT) => '┐',
            dirs if dirs == (RAIL_UP | RAIL_RIGHT) => '└',
            dirs if dirs == (RAIL_UP | RAIL_LEFT) => '┘',
            dirs if dirs == (RAIL_UP | RAIL_DOWN | RAIL_RIGHT) => '├',
            dirs if dirs == (RAIL_UP | RAIL_DOWN | RAIL_LEFT) => '┤',
            dirs if dirs == (RAIL_LEFT | RAIL_RIGHT | RAIL_DOWN) => '┬',
            dirs if dirs == (RAIL_LEFT | RAIL_RIGHT | RAIL_UP) => '┴',
            dirs if dirs == (RAIL_UP | RAIL_DOWN | RAIL_LEFT | RAIL_RIGHT) => '┼',
            dirs if dirs & (RAIL_LEFT | RAIL_RIGHT) != 0 => '─',
            _ => '│',
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct JumpRailEdge {
    source_y: usize,
    target_y: usize,
}

impl JumpRailEdge {
    fn range(self) -> (usize, usize) {
        if self.source_y < self.target_y {
            (self.source_y, self.target_y)
        } else {
            (self.target_y, self.source_y)
        }
    }

    fn span(self) -> usize {
        let (start, end) = self.range();
        end - start
    }
}

fn row_index_containing_virtual_address(rows: &[DisasmRow], virtual_address: u64) -> Option<usize> {
    rows.iter().position(|row| {
        let Some(row_va) = row.virtual_address else {
            return false;
        };
        let row_len = row.len() as u64;
        virtual_address >= row_va && virtual_address < row_va.saturating_add(row_len)
    })
}

fn offscreen_marker(source_va: Option<u64>, target_va: u64) -> char {
    match source_va {
        Some(source_va) if target_va < source_va => '↖',
        Some(source_va) if target_va > source_va => '↙',
        _ => '←',
    }
}

fn allocate_jump_lane(lane_ranges: &[Vec<(usize, usize)>], edge: &JumpRailEdge) -> Option<usize> {
    let range = edge.range();
    lane_ranges.iter().position(|ranges| {
        ranges
            .iter()
            .all(|existing| !ranges_overlap(*existing, range))
    })
}

fn ranges_overlap(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 <= right.1 && right.0 <= left.1
}

fn draw_jump_edge(
    cells: &mut [Vec<JumpRailCell>],
    text_widths: &[usize],
    edge: JumpRailEdge,
    lane: usize,
    width: usize,
) -> bool {
    let source_above_target = edge.source_y < edge.target_y;
    let (start, end) = edge.range();
    let max_text_width = text_widths
        .get(start..=end)
        .and_then(|widths| widths.iter().copied().max())
        .unwrap_or_default();
    let rail_col = max_text_width
        .saturating_add(JUMP_RAIL_INNER_OFFSET)
        .saturating_add(lane * JUMP_RAIL_LANE_WIDTH);
    if rail_col >= width {
        return false;
    }

    for row in start + 1..end {
        add_rail_dirs(cells, row, rail_col, RAIL_UP | RAIL_DOWN);
    }

    let source_start = rail_start_col(
        text_widths.get(edge.source_y).copied().unwrap_or_default(),
        width,
    )
    .min(rail_col);
    let target_start = rail_start_col(
        text_widths.get(edge.target_y).copied().unwrap_or_default(),
        width,
    )
    .min(rail_col);

    if source_above_target {
        draw_source_connector(cells, edge.source_y, source_start, rail_col, RAIL_DOWN);
        draw_target_connector(cells, edge.target_y, target_start, rail_col, RAIL_UP);
    } else {
        draw_source_connector(cells, edge.source_y, source_start, rail_col, RAIL_UP);
        draw_target_connector(cells, edge.target_y, target_start, rail_col, RAIL_DOWN);
    }
    true
}

fn rail_start_col(text_width: usize, width: usize) -> usize {
    text_width.saturating_add(1).min(width.saturating_sub(1))
}

fn draw_source_connector(
    cells: &mut [Vec<JumpRailCell>],
    row: usize,
    start_col: usize,
    rail_col: usize,
    vertical_dir: u8,
) {
    if start_col < rail_col {
        add_rail_dirs(cells, row, start_col, RAIL_RIGHT);
        for col in start_col + 1..rail_col {
            add_rail_dirs(cells, row, col, RAIL_LEFT | RAIL_RIGHT);
        }
    }
    add_rail_dirs(cells, row, rail_col, RAIL_LEFT | vertical_dir);
}

fn draw_target_connector(
    cells: &mut [Vec<JumpRailCell>],
    row: usize,
    start_col: usize,
    rail_col: usize,
    vertical_dir: u8,
) {
    set_rail_arrow_left(cells, row, start_col);
    for col in start_col..rail_col {
        let mut dirs = RAIL_RIGHT;
        if col > start_col {
            dirs |= RAIL_LEFT;
        }
        add_rail_dirs(cells, row, col, dirs);
    }
    add_rail_dirs(cells, row, rail_col, RAIL_LEFT | vertical_dir);
}

fn add_rail_dirs(cells: &mut [Vec<JumpRailCell>], row: usize, col: usize, dirs: u8) {
    if let Some(line) = cells.get_mut(row) {
        if let Some(cell) = line.get_mut(col) {
            cell.add_dirs(dirs);
        }
    }
}

fn set_rail_arrow_left(cells: &mut [Vec<JumpRailCell>], row: usize, col: usize) {
    if let Some(line) = cells.get_mut(row) {
        if let Some(cell) = line.get_mut(col) {
            cell.arrow_left = true;
        }
    }
}

fn set_rail_marker(cells: &mut [Vec<JumpRailCell>], row: usize, col: usize, ch: char) {
    if let Some(line) = cells.get_mut(row) {
        if let Some(cell) = line.get_mut(col) {
            cell.marker = Some(ch);
        }
    }
}

fn merge_jump_rail_line(
    mut text: Line<'static>,
    jump_rail: &Line<'_>,
    palette: &Palette,
) -> Line<'static> {
    let rail_chars = jump_rail
        .spans
        .iter()
        .flat_map(|span| span.content.chars())
        .collect::<Vec<_>>();
    let mut current_width = line_width(&text);
    let mut col = 0;
    while col < rail_chars.len() {
        while col < rail_chars.len() && rail_chars[col] == ' ' {
            col += 1;
        }
        let start = col;
        while col < rail_chars.len() && rail_chars[col] != ' ' {
            col += 1;
        }
        let end = col;
        if start == end || end <= current_width {
            continue;
        }
        let visible_start = start.max(current_width);
        if visible_start > current_width {
            text.spans
                .push(Span::raw(" ".repeat(visible_start - current_width)));
        }
        let segment = rail_chars[visible_start..end].iter().collect::<String>();
        text.spans
            .push(Span::styled(segment, palette.disasm_symbol));
        current_width = end;
    }
    text
}

fn row_function_starts(row: &DisasmRow) -> bool {
    matches!(
        row.function_scope.as_ref().map(|scope| scope.boundary),
        Some(DisasmFunctionBoundary::Entry | DisasmFunctionBoundary::EntryExit)
    )
}

fn row_function_ends(row: &DisasmRow) -> bool {
    matches!(
        row.function_scope.as_ref().map(|scope| scope.boundary),
        Some(DisasmFunctionBoundary::Exit | DisasmFunctionBoundary::EntryExit)
    )
}

fn function_boundary_text(row: &DisasmRow, start: bool, palette: &Palette) -> Line<'static> {
    let Some(scope) = row.function_scope.as_ref() else {
        return Line::raw("");
    };
    let label = if start {
        format!("┌── {}", scope.name)
    } else {
        format!("└── end of {}", scope.name)
    };
    let style = if scope.stale {
        palette.disasm_virtual
    } else {
        palette.disasm_symbol
    };
    Line::from(vec![Span::styled(label, style)])
}

fn build_edit_text(row: &DisasmRow, buffer: &str, cursor: u64, palette: &Palette) -> Line<'static> {
    let style = if row_contains_cursor(row, cursor) {
        palette.cursor.patch(palette.inspector_edit)
    } else {
        palette.inspector_edit
    };
    Line::from(vec![Span::styled(buffer.to_owned(), style)])
}

fn build_instruction_text(row: &DisasmRow, cursor: u64, palette: &Palette) -> Line<'static> {
    let mut parts = row.text.splitn(2, ' ');
    let mnemonic = parts.next().unwrap_or_default();
    let operands = parts.next();
    let active_row = row_contains_cursor(row, cursor);
    let mnemonic_style = if active_row {
        palette.cursor.patch(palette.disasm_mnemonic)
    } else {
        palette.disasm_mnemonic
    };
    let mut spans = function_rail_spans(row, active_row, palette);
    spans.push(Span::styled(mnemonic.to_owned(), mnemonic_style));
    if let Some(operands) = operands {
        spans.push(styled_punctuation(" ", active_row, palette));
        spans.extend(tokenize_operands(row, operands, active_row, palette));
    }
    append_row_suffix(&mut spans, row, active_row, palette);
    Line::from(spans)
}

fn function_rail_spans(row: &DisasmRow, active_row: bool, palette: &Palette) -> Vec<Span<'static>> {
    let Some(scope) = row.function_scope.as_ref() else {
        return Vec::new();
    };
    let marker = match scope.boundary {
        DisasmFunctionBoundary::Entry => "│  ",
        DisasmFunctionBoundary::Body => "│  ",
        DisasmFunctionBoundary::Exit => "│  ",
        DisasmFunctionBoundary::EntryExit => "│  ",
    };
    let base = if scope.stale {
        palette.disasm_virtual
    } else {
        palette.disasm_symbol
    };
    let style = if active_row {
        palette.cursor.patch(base)
    } else {
        base
    };
    vec![Span::styled(marker.to_owned(), style)]
}

fn append_row_suffix(
    spans: &mut Vec<Span<'static>>,
    row: &DisasmRow,
    active_row: bool,
    palette: &Palette,
) {
    if let Some(symbol) = &row.symbol_label {
        spans.push(styled_punctuation(" ", active_row, palette));
        spans.push(styled_operand(
            format!("<{symbol}>"),
            palette.disasm_symbol,
            active_row,
            palette,
        ));
    }
    if let Some(address) = row.virtual_address {
        spans.push(styled_punctuation(" ", active_row, palette));
        spans.push(styled_operand(
            format!("@0x{address:x}"),
            palette.disasm_virtual,
            active_row,
            palette,
        ));
    }
    append_direct_target_suffix(spans, row, active_row, palette);
}

fn append_direct_target_suffix(
    spans: &mut Vec<Span<'static>>,
    row: &DisasmRow,
    active_row: bool,
    palette: &Palette,
) {
    let Some(target) = row.direct_target.as_ref() else {
        return;
    };
    let Some(name) = target.display_name.as_deref() else {
        return;
    };

    spans.push(styled_punctuation(" ", active_row, palette));
    spans.push(styled_punctuation("→", active_row, palette));
    spans.push(styled_punctuation(" ", active_row, palette));
    if row.symbolized_names.iter().any(|symbol| symbol == name) {
        spans.push(styled_operand(
            format!("@0x{:x}", target.virtual_address),
            palette.disasm_virtual,
            active_row,
            palette,
        ));
    } else {
        spans.push(styled_operand(
            format!("<{name}>"),
            palette.disasm_symbol,
            active_row,
            palette,
        ));
    }
}

fn tokenize_operands(
    row: &DisasmRow,
    text: &str,
    active_row: bool,
    palette: &Palette,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for_each_instruction_text_token(text, |token| match token.kind {
        InstructionTextTokenKind::Whitespace => spans.push(styled_operand(
            token.text.to_owned(),
            palette.disasm_operand,
            active_row,
            palette,
        )),
        InstructionTextTokenKind::Punctuation => {
            spans.push(styled_punctuation(token.text, active_row, palette));
        }
        InstructionTextTokenKind::Atom => {
            let base = if row
                .symbolized_names
                .iter()
                .any(|symbol| symbol == token.text)
            {
                palette.disasm_symbol
            } else if looks_like_register(token.text) {
                palette.disasm_register
            } else if looks_like_immediate(token.text) {
                palette.disasm_immediate
            } else {
                palette.disasm_operand
            };
            spans.push(styled_operand(
                token.text.to_owned(),
                base,
                active_row,
                palette,
            ));
        }
    });
    spans
}

fn styled_operand(
    text: String,
    base: ratatui::style::Style,
    active_row: bool,
    palette: &Palette,
) -> Span<'static> {
    let style = if active_row {
        palette.cursor.patch(base)
    } else {
        base
    };
    Span::styled(text, style)
}

fn styled_punctuation(text: &str, active_row: bool, palette: &Palette) -> Span<'static> {
    styled_operand(
        text.to_owned(),
        palette.disasm_punctuation,
        active_row,
        palette,
    )
}

fn row_contains_cursor(row: &DisasmRow, cursor: u64) -> bool {
    let end = row.offset + row.len() as u64 - 1;
    cursor >= row.offset && cursor <= end
}

fn truncate_label(label: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars = label.chars().collect::<Vec<_>>();
    if chars.len() <= width {
        return label.to_owned();
    }
    if width <= 2 {
        return chars.into_iter().take(width).collect();
    }
    let head = width.saturating_sub(1);
    let mut text = chars.into_iter().take(head).collect::<String>();
    text.push('…');
    text
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::{
        build_bytes, build_display, build_gutter, build_jump_rail, build_text,
        visual_row_source_indices, JUMP_RAIL_WIDTH,
    };
    use crate::disasm::{
        DirectBranchKind, DirectBranchTarget, DisasmFunctionBoundary, DisasmFunctionScope,
        DisasmRow, DisasmRowKind, RowBytes,
    };
    use crate::view::palette::{ColorLevel, Palette};

    fn sample_rows() -> Vec<DisasmRow> {
        vec![DisasmRow {
            offset: 0x100,
            virtual_address: Some(0x401000),
            bytes: RowBytes::from_slice(&[0x48, 0x8b, 0x45, 0xf8]),
            text: "mov rax, [rbp - 0x8]".to_owned(),
            assembly_text: "mov rax, [rbp - 0x8]".to_owned(),
            symbolized_names: Vec::new(),
            symbol_label: Some("entry".to_owned()),
            direct_target: Some(crate::disasm::DirectBranchTarget {
                kind: crate::disasm::DirectBranchKind::Call,
                virtual_address: 0x401234,
                display_name: Some("target".to_owned()),
            }),
            function_scope: None,
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }]
    }

    fn line_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn char_at(text: &str, index: usize) -> Option<char> {
        text.chars().nth(index)
    }

    fn char_pos(text: &str, needle: char) -> Option<usize> {
        text.chars().position(|ch| ch == needle)
    }

    fn jump_row(offset: u64, virtual_address: u64, text: &str, target: Option<u64>) -> DisasmRow {
        DisasmRow {
            offset,
            virtual_address: Some(virtual_address),
            bytes: RowBytes::from_slice(&[0x90, 0x90]),
            text: text.to_owned(),
            assembly_text: text.to_owned(),
            symbolized_names: Vec::new(),
            symbol_label: None,
            direct_target: target.map(|virtual_address| DirectBranchTarget {
                kind: DirectBranchKind::Jump,
                virtual_address,
                display_name: None,
            }),
            function_scope: None,
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }
    }

    #[test]
    fn gutter_highlights_active_row() {
        let palette = Palette::new(ColorLevel::Basic);
        let lines = build_gutter(&sample_rows(), 18, 0x101, &palette);
        assert_eq!(lines[0].spans[0].style.bg, palette.cursor.bg);
    }

    #[test]
    fn bytes_highlight_current_byte() {
        let palette = Palette::new(ColorLevel::Basic);
        let lines = build_bytes(&sample_rows(), 0x102, &palette);
        assert!(lines[0]
            .spans
            .iter()
            .any(|span| span.style.bg == palette.cursor.bg));
    }

    #[test]
    fn instruction_text_uses_multiple_operand_styles() {
        let palette = Palette::new(ColorLevel::Basic);
        let lines = build_text(&sample_rows(), 0x100, None, &palette);
        let line = &lines[0];
        assert!(line
            .spans
            .iter()
            .any(|span| span.style.fg == palette.disasm_register.fg));
        assert!(line
            .spans
            .iter()
            .any(|span| span.style.fg == palette.disasm_immediate.fg));
        assert!(line
            .spans
            .iter()
            .any(|span| span.style.fg == palette.disasm_punctuation.fg));
        assert!(line
            .spans
            .iter()
            .any(|span| span.style.fg == palette.disasm_symbol.fg));
        assert!(line
            .spans
            .iter()
            .any(|span| span.content.contains("@0x401000")));
        assert!(line.spans.iter().any(|span| span.content.contains("→")));
        assert!(line
            .spans
            .iter()
            .any(|span| span.content.contains("<target>")));
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn instruction_text_uses_target_address_when_operand_is_already_symbolized() {
        let palette = Palette::new(ColorLevel::Basic);
        let rows = vec![DisasmRow {
            offset: 0x100,
            virtual_address: Some(0x401000),
            bytes: RowBytes::from_slice(&[0xe8, 0xfb, 0x0f, 0x00, 0x00]),
            text: "call entry".to_owned(),
            assembly_text: "call 0x402000".to_owned(),
            symbolized_names: vec!["entry".to_owned()],
            symbol_label: None,
            direct_target: Some(crate::disasm::DirectBranchTarget {
                kind: crate::disasm::DirectBranchKind::Call,
                virtual_address: 0x402000,
                display_name: Some("entry".to_owned()),
            }),
            function_scope: None,
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }];

        let lines = build_text(&rows, 0x100, None, &palette);
        let line = &lines[0];
        assert!(line.spans.iter().any(|span| span.content.contains("→")));
        assert!(line
            .spans
            .iter()
            .any(|span| span.content.contains("@0x402000")));
        assert!(!line
            .spans
            .iter()
            .any(|span| span.content.contains("<entry>")));
    }

    #[test]
    fn instruction_text_colors_symbolized_operands() {
        let palette = Palette::new(ColorLevel::Basic);
        let rows = vec![DisasmRow {
            offset: 0x100,
            virtual_address: Some(0x401000),
            bytes: RowBytes::from_slice(&[0xe8, 0xfb, 0x0f, 0x00, 0x00]),
            text: "call entry".to_owned(),
            assembly_text: "call 0x402000".to_owned(),
            symbolized_names: vec!["entry".to_owned()],
            symbol_label: None,
            direct_target: None,
            function_scope: None,
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }];

        let lines = build_text(&rows, 0x100, None, &palette);
        assert!(lines[0]
            .spans
            .iter()
            .any(|span| span.content == "entry" && span.style.fg == palette.disasm_symbol.fg));
    }

    #[test]
    fn function_scope_renders_box_drawing_body_rail_prefix() {
        let palette = Palette::new(ColorLevel::Basic);
        let mut rows = sample_rows();
        rows[0].function_scope = Some(DisasmFunctionScope {
            name: "sub_401000".to_owned(),
            entry_va: 0x401000,
            boundary: DisasmFunctionBoundary::Body,
            stale: false,
        });

        let lines = build_text(&rows, 0x100, None, &palette);

        assert_eq!(lines[0].spans[0].content.as_ref(), "│  ");
        assert_eq!(lines[0].spans[0].style.fg, palette.disasm_symbol.fg);
        assert!(lines[0].spans.iter().any(|span| span.content == "mov"));
    }

    #[test]
    fn function_scope_renders_named_boundary_lines() {
        let palette = Palette::new(ColorLevel::Basic);
        let mut rows = sample_rows();
        rows[0].function_scope = Some(DisasmFunctionScope {
            name: "sub_401000".to_owned(),
            entry_va: 0x401000,
            boundary: DisasmFunctionBoundary::EntryExit,
            stale: false,
        });

        let gutter = build_gutter(&rows, 18, 0x100, &palette);
        let bytes = build_bytes(&rows, 0x100, &palette);
        let text = build_text(&rows, 0x100, None, &palette);
        let sources = visual_row_source_indices(&rows);

        assert_eq!(gutter.len(), 3);
        assert_eq!(bytes.len(), 3);
        assert_eq!(text.len(), 3);
        assert_eq!(sources, vec![None, Some(0), None]);
        assert_eq!(line_text(&text[0]), "┌── sub_401000");
        assert_eq!(line_text(&text[1]).chars().next(), Some('│'));
        assert_eq!(line_text(&text[2]), "└── end of sub_401000");
    }

    #[test]
    fn display_wraps_function_rows_with_rail_continuation() {
        let palette = Palette::new(ColorLevel::Basic);
        let mut rows = sample_rows();
        rows[0].text = "nop word ptr cs [rax + rax]".to_owned();
        rows[0].assembly_text = rows[0].text.clone();
        rows[0].function_scope = Some(DisasmFunctionScope {
            name: "sub_401000".to_owned(),
            entry_va: 0x401000,
            boundary: DisasmFunctionBoundary::Body,
            stale: false,
        });

        let display = build_display(&rows, 18, 0x100, None, 12, &palette);

        assert!(display.text.len() > 1);
        assert_eq!(display.gutter.len(), display.text.len());
        assert_eq!(display.bytes.len(), display.text.len());
        assert_eq!(display.row_sources, vec![Some(0); display.text.len()]);
        for line in &display.text {
            assert!(line_text(line).starts_with("│  "));
        }
        for line in display.text.iter().skip(1) {
            assert!(line.spans[0].content.as_ref().starts_with("│  "));
            assert_eq!(line.spans[0].style.fg, palette.disasm_symbol.fg);
        }
    }

    #[test]
    fn jump_rail_draws_visible_direct_jump_on_right() {
        let palette = Palette::new(ColorLevel::Basic);
        let rows = vec![
            jump_row(0x100, 0x401000, "je 0x401004", Some(0x401004)),
            jump_row(0x102, 0x401002, "nop", None),
            jump_row(0x104, 0x401004, "ret", None),
        ];
        let display = build_display(&rows, 18, 0x100, None, 80, &palette);
        let rail = build_jump_rail(
            &rows,
            &display.row_sources,
            &display.text,
            80 + JUMP_RAIL_WIDTH,
            &palette,
        );
        let text = rail.iter().map(line_text).collect::<Vec<_>>();
        let source_start = line_text(&display.text[0]).chars().count() + 1;
        let target_start = line_text(&display.text[2]).chars().count() + 1;
        let rail_col = char_pos(&text[0], '┐').unwrap();

        assert_eq!(rail.len(), display.text.len());
        assert_eq!(char_at(&text[0], source_start), Some('─'));
        assert_eq!(char_at(&text[0], source_start + 1), Some('─'));
        assert_eq!(char_at(&text[1], rail_col), Some('│'));
        assert_eq!(char_at(&text[2], target_start), Some('◀'));
        assert_eq!(char_at(&text[2], target_start + 1), Some('─'));
        assert_eq!(char_at(&text[2], rail_col), Some('┘'));
    }

    #[test]
    fn jump_rail_places_overlapping_jumps_in_separate_lanes() {
        let palette = Palette::new(ColorLevel::Basic);
        let rows = vec![
            jump_row(0x100, 0x401000, "je 0x401006", Some(0x401006)),
            jump_row(0x102, 0x401002, "je 0x401004", Some(0x401004)),
            jump_row(0x104, 0x401004, "nop", None),
            jump_row(0x106, 0x401006, "ret", None),
        ];
        let display = build_display(&rows, 18, 0x100, None, 80, &palette);
        let rail = build_jump_rail(
            &rows,
            &display.row_sources,
            &display.text,
            80 + JUMP_RAIL_WIDTH,
            &palette,
        );
        let text = rail.iter().map(line_text).collect::<Vec<_>>();
        let long_rail_col = char_pos(&text[0], '┐').unwrap();
        let short_rail_col = char_pos(&text[1], '┐').unwrap();

        assert!(short_rail_col < long_rail_col);
        assert_eq!(char_at(&text[1], short_rail_col), Some('┐'));
        assert_eq!(char_at(&text[1], long_rail_col), Some('│'));
        let short_target_start = line_text(&display.text[2]).chars().count() + 1;
        assert_eq!(char_at(&text[2], short_target_start), Some('◀'));
        assert_eq!(char_at(&text[2], short_rail_col), Some('┘'));
        assert_eq!(char_at(&text[2], long_rail_col), Some('│'));
        assert_eq!(char_at(&text[3], long_rail_col), Some('┘'));
    }

    #[test]
    fn jump_rail_marks_crossing_connectors() {
        let palette = Palette::new(ColorLevel::Basic);
        let rows = vec![
            jump_row(0x100, 0x401000, "je 0x401008", Some(0x401008)),
            jump_row(0x102, 0x401002, "nop", None),
            jump_row(0x104, 0x401004, "je 0x40100a", Some(0x40100a)),
            jump_row(0x106, 0x401006, "nop", None),
            jump_row(0x108, 0x401008, "nop", None),
            jump_row(0x10a, 0x40100a, "ret", None),
        ];
        let display = build_display(&rows, 18, 0x100, None, 80, &palette);
        let rail = build_jump_rail(
            &rows,
            &display.row_sources,
            &display.text,
            80 + JUMP_RAIL_WIDTH,
            &palette,
        );
        let text = rail.iter().map(line_text).collect::<Vec<_>>();
        let inner_rail_col = char_pos(&text[2], '┐').unwrap();

        assert_eq!(char_at(&text[4], inner_rail_col), Some('┼'));
    }
}
