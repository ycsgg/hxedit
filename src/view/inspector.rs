use ratatui::text::{Line, Span};

use crate::format::parse::InspectorRow;
use crate::view::palette::Palette;

pub const FIELD_NAME_MIN_WIDTH: u16 = 16;
pub const FIELD_NAME_MAX_WIDTH_RATIO: f64 = 0.55; // 字段名最多占 inspector 宽度的55%
pub const FIELD_NAME_ABSOLUTE_MAX: usize = 48; // 字段名绝对最大宽度
pub const FIELD_VALUE_MIN_WIDTH: usize = 18; // 字段值至少保留的可读宽度

/// 计算字段名区域的最大宽度（动态，基于终端宽度）
fn calculate_field_name_width(total_width: usize) -> usize {
    let width_after_value_reserve = total_width.saturating_sub(FIELD_VALUE_MIN_WIDTH);
    let dynamic_max = (total_width as f64 * FIELD_NAME_MAX_WIDTH_RATIO).ceil() as usize;
    dynamic_max
        .min(width_after_value_reserve.max(FIELD_NAME_MIN_WIDTH as usize))
        .max(FIELD_NAME_MIN_WIDTH as usize)
        .min(FIELD_NAME_ABSOLUTE_MAX)
}

#[derive(Debug, Clone)]
pub struct RenderedInspectorLine {
    pub row_index: usize,
    pub line: Line<'static>,
    pub cursor_col: Option<u16>,
}

/// 字段渲染配置
#[derive(Debug, Clone)]
struct FieldRenderConfig {
    width: usize,
    max_name_len: usize,
    field_name_area_width: usize,
}

impl FieldRenderConfig {
    fn new(width: usize, max_name_len: usize, field_name_area_width: usize) -> Self {
        Self {
            width,
            max_name_len,
            field_name_area_width,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum WrapMode {
    Natural,
    Identifier,
}

#[derive(Debug, Clone)]
struct WrappedChunk {
    text: String,
    start_char: usize,
    end_char: usize,
}

pub fn build_wrapped(
    state_rows: &[InspectorRow],
    selected_row: usize,
    editing: Option<(&str, usize)>,
    width: u16,
    palette: &Palette,
) -> Vec<RenderedInspectorLine> {
    let width = width.max(1) as usize;

    // 计算字段名区域的最大宽度（动态，基于终端宽度）
    let field_name_area_width = calculate_field_name_width(width);

    // First pass: 找出所有字段名中最长的那个（用于对齐）
    let mut max_name_len = FIELD_NAME_MIN_WIDTH as usize;
    for row in state_rows {
        if let InspectorRow::Field { name, depth, .. } = row {
            let wrapped_name_lines = wrap_field_name_lines(name, *depth, field_name_area_width);
            let rendered_width = wrapped_name_lines
                .iter()
                .map(|line| char_count(line))
                .max()
                .unwrap_or_default();
            max_name_len = max_name_len.max(rendered_width.min(field_name_area_width));
        }
    }

    max_name_len += 1; // 字段名和字段值之间至少留1个字符的间隔

    let mut out = Vec::new();

    for (row_index, row) in state_rows.iter().enumerate() {
        match row {
            InspectorRow::Header {
                name,
                depth,
                collapsed,
                has_children,
                ..
            } => {
                let indent = "  ".repeat(*depth);
                let fold_indicator = if *has_children {
                    if *collapsed {
                        "▶ "
                    } else {
                        "▼ "
                    }
                } else {
                    "  "
                };
                let is_selected = row_index == selected_row;
                let header_style = if is_selected && *has_children {
                    palette.inspector_active
                } else {
                    palette.inspector_header
                };
                for chunk in wrap_header_lines(name, &indent, fold_indicator, width) {
                    out.push(RenderedInspectorLine {
                        row_index,
                        line: Line::styled(chunk, header_style),
                        cursor_col: None,
                    });
                }
            }
            InspectorRow::Field {
                name,
                display,
                depth,
                ..
            } => {
                let name_lines = wrap_field_name_lines(name, *depth, field_name_area_width);
                let is_selected = row_index == selected_row;
                let (value_text, value_style, cursor_pos) = if is_selected {
                    if let Some((buffer, cursor_pos)) = editing {
                        (buffer.to_owned(), palette.inspector_edit, Some(cursor_pos))
                    } else {
                        (display.clone(), palette.inspector_active, None)
                    }
                } else {
                    (display.clone(), palette.inspector_value, None)
                };
                let name_style = if is_selected {
                    palette.inspector_active
                } else {
                    palette.inspector_field
                };

                out.extend(wrap_field(
                    row_index,
                    &name_lines,
                    &value_text,
                    name_style,
                    value_style,
                    cursor_pos,
                    FieldRenderConfig::new(width, max_name_len, field_name_area_width),
                ));
            }
        }
    }

    out
}

fn wrap_field(
    row_index: usize,
    name_lines: &[String],
    value_text: &str,
    name_style: ratatui::style::Style,
    value_style: ratatui::style::Style,
    cursor_pos: Option<usize>,
    config: FieldRenderConfig,
) -> Vec<RenderedInspectorLine> {
    let name_str = name_lines.first().map(String::as_str).unwrap_or_default();
    let name_len = char_count(name_str);
    let width = config.width;
    let max_name_len = config.max_name_len;
    let field_name_area_width = config.field_name_area_width;

    // 如果字段名超过最大宽度，需要折行显示字段名
    if name_lines.len() > 1 || name_len > field_name_area_width {
        // 字段名折行：字段名单独占几行，值从新行开始
        let value_start_col = max_name_len.min(width.saturating_sub(1));
        let value_width = width.saturating_sub(value_start_col).max(1);

        let mut out = Vec::new();

        // 渲染字段名的每一行
        for (i, name_chunk) in name_lines.iter().enumerate() {
            let is_last_name_line = i == name_lines.len() - 1;
            out.push(RenderedInspectorLine {
                row_index,
                line: Line::styled(
                    if is_last_name_line {
                        // 最后一行字段名后用空格填充到对齐位置
                        format!("{:<width$}", name_chunk, width = max_name_len)
                    } else {
                        // 字段名折行，只显示字段名
                        name_chunk.clone()
                    },
                    name_style,
                ),
                cursor_col: None,
            });
        }

        // 渲染字段值
        let value_chunks = wrap_text(value_text, value_width, WrapMode::Natural);
        let cursor_char_index =
            cursor_pos.map(|pos| char_count(&value_text[..pos.min(value_text.len())]));

        for chunk in &value_chunks {
            let cursor_col = cursor_char_index.and_then(|cursor| {
                (cursor >= chunk.start_char && cursor <= chunk.end_char).then(|| {
                    value_start_col as u16
                        + (cursor
                            .saturating_sub(chunk.start_char)
                            .min(char_count(&chunk.text))) as u16
                })
            });

            out.push(RenderedInspectorLine {
                row_index,
                line: Line::from(vec![
                    Span::styled(" ".repeat(max_name_len), name_style),
                    Span::styled(chunk.text.clone(), value_style),
                ]),
                cursor_col,
            });
        }

        out
    } else {
        // 字段名不需要折行，正常渲染
        let prefix_width = max_name_len.min(width.saturating_sub(1).max(1));
        let value_width = width.saturating_sub(prefix_width).max(1);

        let prefix_name = truncate_chars(name_str, prefix_width);
        let first_prefix = format!("{:<width$}", prefix_name, width = prefix_width);
        let continuation_prefix = " ".repeat(prefix_width);
        let value_chunks = wrap_text(value_text, value_width, WrapMode::Natural);

        let cursor_char_index =
            cursor_pos.map(|pos| char_count(&value_text[..pos.min(value_text.len())]));
        let mut out = Vec::with_capacity(value_chunks.len().max(1));

        for (chunk_index, chunk) in value_chunks.iter().enumerate() {
            let prefix = if chunk_index == 0 {
                first_prefix.clone()
            } else {
                continuation_prefix.clone()
            };
            let cursor_col = cursor_char_index.and_then(|cursor| {
                (cursor >= chunk.start_char && cursor <= chunk.end_char).then(|| {
                    prefix_width as u16
                        + (cursor
                            .saturating_sub(chunk.start_char)
                            .min(char_count(&chunk.text))) as u16
                })
            });

            out.push(RenderedInspectorLine {
                row_index,
                line: Line::from(vec![
                    Span::styled(prefix, name_style),
                    Span::styled(chunk.text.clone(), value_style),
                ]),
                cursor_col,
            });
        }

        out
    }
}

fn wrap_field_name_lines(name: &str, depth: usize, field_name_area_width: usize) -> Vec<String> {
    let indent = "  ".repeat(depth);
    let indent_width = char_count(&indent);
    let name_width = field_name_area_width.saturating_sub(indent_width).max(1);
    wrap_text(name, name_width, WrapMode::Identifier)
        .into_iter()
        .map(|chunk| format!("{indent}{}", chunk.text))
        .collect()
}

fn wrap_header_lines(name: &str, indent: &str, fold_indicator: &str, width: usize) -> Vec<String> {
    let continuation_prefix = format!("{}{}", indent, " ".repeat(char_count(fold_indicator)));
    let content_width = width
        .saturating_sub(char_count(indent) + char_count(fold_indicator))
        .max(1);
    let chunks = wrap_text(name, content_width, WrapMode::Natural);
    chunks
        .into_iter()
        .enumerate()
        .map(|(idx, chunk)| {
            if idx == 0 {
                format!("{indent}{fold_indicator}{}", chunk.text)
            } else {
                format!("{continuation_prefix}{}", chunk.text)
            }
        })
        .collect()
}

fn wrap_text(text: &str, width: usize, mode: WrapMode) -> Vec<WrappedChunk> {
    if width == 0 {
        return vec![WrappedChunk {
            text: String::new(),
            start_char: 0,
            end_char: 0,
        }];
    }

    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return vec![WrappedChunk {
            text: String::new(),
            start_char: 0,
            end_char: 0,
        }];
    }

    let mut out = Vec::new();
    let mut start = 0;

    while start < chars.len() {
        let remaining = chars.len() - start;
        if remaining <= width {
            out.push(WrappedChunk {
                text: chars[start..].iter().collect(),
                start_char: start,
                end_char: chars.len(),
            });
            break;
        }

        let limit = start + width;
        let end = find_wrap_end(&chars, start, limit, mode)
            .filter(|end| *end > start)
            .unwrap_or(limit);

        out.push(WrappedChunk {
            text: chars[start..end].iter().collect(),
            start_char: start,
            end_char: end,
        });

        start = end;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }

    if out.is_empty() {
        out.push(WrappedChunk {
            text: String::new(),
            start_char: 0,
            end_char: 0,
        });
    }

    out
}

fn find_wrap_end(chars: &[char], start: usize, limit: usize, mode: WrapMode) -> Option<usize> {
    let mut whitespace_end = None;
    let mut separator_end = None;
    let mut identifier_end = None;

    for idx in start..limit {
        let ch = chars[idx];
        if ch.is_whitespace() {
            whitespace_end = Some(idx + 1);
            continue;
        }

        match mode {
            WrapMode::Natural => {
                if is_soft_separator(ch) {
                    separator_end = Some(idx + 1);
                }
                if is_identifier_boundary(chars, idx) {
                    identifier_end = Some(idx);
                }
            }
            WrapMode::Identifier => {
                if is_identifier_delimiter(ch) {
                    separator_end = Some(idx + 1);
                }
                if is_identifier_boundary(chars, idx) {
                    identifier_end = Some(idx);
                }
            }
        }
    }

    whitespace_end.or(separator_end).or(identifier_end)
}

fn is_soft_separator(ch: char) -> bool {
    matches!(
        ch,
        '|' | ',' | ';' | ':' | ')' | ']' | '}' | '/' | '\\' | '_' | '-'
    )
}

fn is_identifier_delimiter(ch: char) -> bool {
    matches!(ch, '_' | '-' | ':' | '/' | '\\' | '.')
}

fn is_identifier_boundary(chars: &[char], idx: usize) -> bool {
    if idx == 0 || idx >= chars.len() {
        return false;
    }

    let prev = chars[idx - 1];
    let current = chars[idx];
    let next = chars.get(idx + 1).copied();

    (current.is_uppercase()
        && (prev.is_lowercase()
            || prev.is_ascii_digit()
            || (prev.is_uppercase() && next.is_some_and(|ch| ch.is_lowercase()))))
        || (current.is_ascii_digit() && prev.is_alphabetic())
}

fn truncate_chars(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn field_value_wraps_to_multiple_visual_lines() {
        let lines = build_wrapped(
            &[InspectorRow::Field {
                field_index: 0,
                name: "name".into(),
                display: "abcdefghijklmnopqrstuvwxyz".into(),
                abs_offset: 0,
                size: 1,
                depth: 1,
                editable: true,
            }],
            0,
            None,
            20,
            &Palette::new(ColorLevel::Basic),
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.row_index == 0));
    }

    #[test]
    fn identifier_wrap_prefers_camel_case_boundaries() {
        let chunks = wrap_text("PointerToRelocations", 12, WrapMode::Identifier);
        let rendered = chunks
            .into_iter()
            .map(|chunk| chunk.text)
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec!["PointerTo".to_string(), "Relocations".to_string()]
        );
    }

    #[test]
    fn natural_wrap_prefers_separator_boundaries() {
        let chunks = wrap_text(
            "0x42000040 [CNT_INITIALIZED_DATA | DISCARDABLE | READ]",
            31,
            WrapMode::Natural,
        );
        let rendered = chunks
            .into_iter()
            .map(|chunk| chunk.text)
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec![
                "0x42000040 ".to_string(),
                "[CNT_INITIALIZED_DATA | ".to_string(),
                "DISCARDABLE | READ]".to_string(),
            ]
        );
    }

    #[test]
    fn wrapped_field_name_keeps_nested_indent_on_continuation_lines() {
        let lines = build_wrapped(
            &[InspectorRow::Field {
                field_index: 0,
                name: "PointerToRelocations".into(),
                display: "0x00000000".into(),
                abs_offset: 0,
                size: 4,
                depth: 3,
                editable: true,
            }],
            0,
            None,
            24,
            &Palette::new(ColorLevel::Basic),
        );

        let rendered = lines
            .iter()
            .map(|line| {
                line.line
                    .spans
                    .iter()
                    .map(|span| span.content.clone())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(rendered[0].starts_with("      "));
        assert!(rendered[1].starts_with("      "));
    }

    #[test]
    fn editing_cursor_can_move_to_wrapped_line() {
        let lines = build_wrapped(
            &[InspectorRow::Field {
                field_index: 0,
                name: "name".into(),
                display: String::new(),
                abs_offset: 0,
                size: 1,
                depth: 1,
                editable: true,
            }],
            0,
            Some(("abcdefghijklmnop", 12)),
            20,
            &Palette::new(ColorLevel::Basic),
        );
        assert!(lines.iter().skip(1).any(|line| line.cursor_col.is_some()));
    }

    #[test]
    fn editing_cursor_on_first_wrapped_line_does_not_panic() {
        let lines = build_wrapped(
            &[InspectorRow::Field {
                field_index: 0,
                name: "name".into(),
                display: String::new(),
                abs_offset: 0,
                size: 1,
                depth: 1,
                editable: true,
            }],
            0,
            Some(("abcdefghijklmnop", 2)),
            20,
            &Palette::new(ColorLevel::Basic),
        );
        assert!(lines.iter().any(|line| line.cursor_col.is_some()));
    }

    #[test]
    fn collapsed_header_renders_right_arrow_indicator() {
        let lines = build_wrapped(
            &[InspectorRow::Header {
                name: "Section".into(),
                depth: 0,
                node_path: vec![("Section".into(), 0)],
                collapsed: true,
                has_children: true,
            }],
            0,
            None,
            40,
            &Palette::new(ColorLevel::Basic),
        );
        let text = lines
            .iter()
            .map(|line| {
                line.line
                    .spans
                    .iter()
                    .map(|s| s.content.clone())
                    .collect::<String>()
            })
            .collect::<String>();
        assert!(text.contains("▶"), "expected `▶` in {:?}", text);
        assert!(text.contains("Section"));
    }

    #[test]
    fn expanded_header_renders_down_arrow_indicator() {
        let lines = build_wrapped(
            &[InspectorRow::Header {
                name: "Section".into(),
                depth: 0,
                node_path: vec![("Section".into(), 0)],
                collapsed: false,
                has_children: true,
            }],
            0,
            None,
            40,
            &Palette::new(ColorLevel::Basic),
        );
        let text = lines
            .iter()
            .map(|line| {
                line.line
                    .spans
                    .iter()
                    .map(|s| s.content.clone())
                    .collect::<String>()
            })
            .collect::<String>();
        assert!(text.contains("▼"), "expected `▼` in {:?}", text);
    }

    #[test]
    fn header_without_children_renders_no_arrow() {
        let lines = build_wrapped(
            &[InspectorRow::Header {
                name: "Empty".into(),
                depth: 0,
                node_path: vec![("Empty".into(), 0)],
                collapsed: false,
                has_children: false,
            }],
            0,
            None,
            40,
            &Palette::new(ColorLevel::Basic),
        );
        let text = lines
            .iter()
            .map(|line| {
                line.line
                    .spans
                    .iter()
                    .map(|s| s.content.clone())
                    .collect::<String>()
            })
            .collect::<String>();
        assert!(!text.contains("▶"));
        assert!(!text.contains("▼"));
        assert!(text.contains("Empty"));
    }
}
