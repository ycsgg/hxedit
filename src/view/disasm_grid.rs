use ratatui::text::{Line, Span};

use crate::disasm::text::{
    looks_like_immediate, looks_like_register, tokenize_instruction_text, InstructionTextTokenKind,
};
use crate::disasm::{DisasmRow, DisasmRowKind};
use crate::view::palette::Palette;

pub fn build_gutter(
    rows: &[DisasmRow],
    width: usize,
    cursor: u64,
    palette: &Palette,
) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| {
            let label_style = if row_contains_cursor(row, cursor) {
                palette.cursor.patch(palette.disasm_label)
            } else {
                palette.disasm_label
            };
            Line::from(vec![Span::styled(
                truncate_label(&row.label(), width),
                label_style,
            )])
        })
        .collect()
}

pub fn build_bytes(rows: &[DisasmRow], cursor: u64, palette: &Palette) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| {
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
        })
        .collect()
}

pub fn build_text(rows: &[DisasmRow], cursor: u64, palette: &Palette) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| match row.kind {
            DisasmRowKind::Instruction => build_instruction_text(row, cursor, palette),
            DisasmRowKind::Data => {
                let style = if row_contains_cursor(row, cursor) {
                    palette.cursor.patch(palette.disasm_data)
                } else {
                    palette.disasm_data
                };
                let mut spans = vec![Span::styled(row.text.clone(), style)];
                append_row_suffix(&mut spans, row, row_contains_cursor(row, cursor), palette);
                Line::from(spans)
            }
            DisasmRowKind::Invalid => {
                let style = if row_contains_cursor(row, cursor) {
                    palette.cursor.patch(palette.warning)
                } else {
                    palette.warning
                };
                let mut spans = vec![Span::styled(row.text.clone(), style)];
                append_row_suffix(&mut spans, row, row_contains_cursor(row, cursor), palette);
                Line::from(spans)
            }
        })
        .collect()
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
    let mut spans = vec![Span::styled(mnemonic.to_owned(), mnemonic_style)];
    if let Some(operands) = operands {
        spans.push(styled_punctuation(" ", active_row, palette));
        spans.extend(tokenize_operands(row, operands, active_row, palette));
    }
    append_row_suffix(&mut spans, row, active_row, palette);
    Line::from(spans)
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

    use super::{build_bytes, build_gutter, build_text};
    use crate::disasm::{DisasmRow, DisasmRowKind};
    use crate::view::palette::{ColorLevel, Palette};

    fn sample_rows() -> Vec<DisasmRow> {
        vec![DisasmRow {
            offset: 0x100,
            virtual_address: Some(0x401000),
            bytes: vec![0x48, 0x8b, 0x45, 0xf8],
            text: "mov rax, [rbp - 0x8]".to_owned(),
            symbolized_names: Vec::new(),
            symbol_label: Some("entry".to_owned()),
            direct_target: Some(crate::disasm::DirectBranchTarget {
                kind: crate::disasm::DirectBranchKind::Call,
                virtual_address: 0x401234,
                display_name: Some("target".to_owned()),
            }),
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }]
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
        let lines = build_text(&sample_rows(), 0x100, &palette);
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
            symbolized_names: vec!["entry".to_owned()],
            symbol_label: None,
            direct_target: Some(crate::disasm::DirectBranchTarget {
                kind: crate::disasm::DirectBranchKind::Call,
                virtual_address: 0x402000,
                display_name: Some("entry".to_owned()),
            }),
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }];

        let lines = build_text(&rows, 0x100, &palette);
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
            symbolized_names: vec!["entry".to_owned()],
            symbol_label: None,
            direct_target: None,
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }];

        let lines = build_text(&rows, 0x100, &palette);
        assert!(lines[0]
            .spans
            .iter()
            .any(|span| span.content == "entry" && span.style.fg == palette.disasm_symbol.fg));
    }
}
