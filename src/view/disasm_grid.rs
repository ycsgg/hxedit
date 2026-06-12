use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::disasm::text::{
    looks_like_immediate, looks_like_register, tokenize_instruction_text, InstructionTextTokenKind,
};
use crate::disasm::{DisasmFunctionBoundary, DisasmRow, DisasmRowKind};
use crate::view::palette::Palette;

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
    for token in tokenize_instruction_text(text) {
        match token.kind {
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
        }
    }
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

    use super::{build_bytes, build_display, build_gutter, build_text, visual_row_source_indices};
    use crate::disasm::{DisasmFunctionBoundary, DisasmFunctionScope, DisasmRow, DisasmRowKind};
    use crate::view::palette::{ColorLevel, Palette};

    fn sample_rows() -> Vec<DisasmRow> {
        vec![DisasmRow {
            offset: 0x100,
            virtual_address: Some(0x401000),
            bytes: vec![0x48, 0x8b, 0x45, 0xf8],
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
            bytes: vec![0xe8, 0xfb, 0x0f, 0x00, 0x00],
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
            bytes: vec![0xe8, 0xfb, 0x0f, 0x00, 0x00],
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
}
