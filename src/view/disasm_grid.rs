use ratatui::text::{Line, Span};

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
                Line::from(vec![Span::styled(row.text.clone(), style)])
            }
            DisasmRowKind::Invalid => {
                let style = if row_contains_cursor(row, cursor) {
                    palette.cursor.patch(palette.warning)
                } else {
                    palette.warning
                };
                Line::from(vec![Span::styled(row.text.clone(), style)])
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
        spans.extend(tokenize_operands(operands, active_row, palette));
    }
    Line::from(spans)
}

fn tokenize_operands(text: &str, active_row: bool, palette: &Palette) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < chars.len() {
        let ch = chars[idx];
        if ch.is_whitespace() {
            let mut end = idx + 1;
            while end < chars.len() && chars[end].is_whitespace() {
                end += 1;
            }
            spans.push(styled_operand(
                chars[idx..end].iter().collect::<String>(),
                palette.disasm_operand,
                active_row,
                palette,
            ));
            idx = end;
            continue;
        }
        if is_punctuation(ch) {
            spans.push(styled_punctuation(&ch.to_string(), active_row, palette));
            idx += 1;
            continue;
        }

        let mut end = idx + 1;
        while end < chars.len() && !chars[end].is_whitespace() && !is_punctuation(chars[end]) {
            end += 1;
        }
        let token = chars[idx..end].iter().collect::<String>();
        let base = if looks_like_register(&token) {
            palette.disasm_register
        } else if looks_like_immediate(&token) {
            palette.disasm_immediate
        } else {
            palette.disasm_operand
        };
        spans.push(styled_operand(token, base, active_row, palette));
        idx = end;
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

fn looks_like_register(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| ch == '%' || ch == '#');
    let lower = token.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "al" | "ah"
            | "ax"
            | "eax"
            | "rax"
            | "bl"
            | "bh"
            | "bx"
            | "ebx"
            | "rbx"
            | "cl"
            | "ch"
            | "cx"
            | "ecx"
            | "rcx"
            | "dl"
            | "dh"
            | "dx"
            | "edx"
            | "rdx"
            | "si"
            | "esi"
            | "rsi"
            | "di"
            | "edi"
            | "rdi"
            | "bp"
            | "ebp"
            | "rbp"
            | "sp"
            | "esp"
            | "rsp"
            | "ip"
            | "eip"
            | "rip"
            | "pc"
            | "lr"
            | "fp"
            | "xzr"
            | "wzr"
            | "nzcv"
            | "cpsr"
            | "spsr"
    ) || lower.starts_with('r') && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with('x') && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with('w') && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("v") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("q") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("d") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("s") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("zmm")
        || lower.starts_with("ymm")
        || lower.starts_with("xmm")
}

fn looks_like_immediate(token: &str) -> bool {
    let trimmed = token.trim_matches(|ch: char| ch == '#' || ch == '$');
    let trimmed = trimmed.strip_prefix('-').unwrap_or(trimmed);
    trimmed.starts_with("0x")
        || trimmed.chars().all(|ch| ch.is_ascii_digit())
        || trimmed.ends_with('h')
            && trimmed[..trimmed.len().saturating_sub(1)]
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
}

fn is_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ',' | '[' | ']' | '(' | ')' | '{' | '}' | '+' | '-' | '*' | ':' | '!' | '='
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
            bytes: vec![0x48, 0x8b, 0x45, 0xf8],
            text: "mov rax, [rbp - 0x8]".to_owned(),
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
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
    }
}
