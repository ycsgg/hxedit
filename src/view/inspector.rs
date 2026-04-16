use ratatui::text::{Line, Span};

use crate::format::parse::InspectorRow;
use crate::view::palette::Palette;

pub const FIELD_NAME_MIN_WIDTH: u16 = 16;
pub const FIELD_NAME_MAX_WIDTH_RATIO: f64 = 0.4; // 字段名最多占终端宽度的40%
pub const FIELD_NAME_ABSOLUTE_MAX: usize = 30; // 字段名绝对最大宽度

/// 计算字段名区域的最大宽度（动态，基于终端宽度）
fn calculate_field_name_width(total_width: usize) -> usize {
    let dynamic_max = (total_width as f64 * FIELD_NAME_MAX_WIDTH_RATIO).ceil() as usize;
    dynamic_max
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
            let indent = "  ".repeat(*depth);
            let name_str = format!("{}{}", indent, name);
            let name_len = char_count(&name_str);
            // 只统计不超过最大宽度的字段名，用于对齐
            max_name_len = max_name_len.max(name_len.min(field_name_area_width));
        }
    }
    
    max_name_len = max_name_len + 1; // 字段名和字段值之间至少留1个字符的间隔

    let mut out = Vec::new();

    for (row_index, row) in state_rows.iter().enumerate() {
        match row {
            InspectorRow::Header { name, depth } => {
                let indent = "  ".repeat(*depth);
                let title = format!("{}── {} ──", indent, name);
                for chunk in wrap_text(&title, width) {
                    out.push(RenderedInspectorLine {
                        row_index,
                        line: Line::styled(chunk, palette.inspector_header),
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
                let indent = "  ".repeat(*depth);
                let name_str = format!("{}{}", indent, name);
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
                    &name_str,
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
    name_str: &str,
    value_text: &str,
    name_style: ratatui::style::Style,
    value_style: ratatui::style::Style,
    cursor_pos: Option<usize>,
    config: FieldRenderConfig,
) -> Vec<RenderedInspectorLine> {
    let name_len = char_count(name_str);
    let width = config.width;
    let max_name_len = config.max_name_len;
    let field_name_area_width = config.field_name_area_width;

    // 如果字段名超过最大宽度，需要折行显示字段名
    if name_len > field_name_area_width {
        // 字段名折行：字段名单独占几行，值从新行开始
        let name_chunks = wrap_text(name_str, field_name_area_width);
        let value_start_col = max_name_len.min(width.saturating_sub(1));
        let value_width = width.saturating_sub(value_start_col).max(1);

        let mut out = Vec::new();

        // 渲染字段名的每一行
        for (i, name_chunk) in name_chunks.iter().enumerate() {
            let is_last_name_line = i == name_chunks.len() - 1;
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

        // 渲染字段值（如果字段名后还有空间）
        let value_chunks = wrap_text(value_text, value_width);
        let cursor_char_index =
            cursor_pos.map(|pos| char_count(&value_text[..pos.min(value_text.len())]));

        for (chunk_index, chunk) in value_chunks.iter().enumerate() {
            let cursor_col = cursor_char_index.and_then(|cursor| {
                let line_start = chunk_index * value_width;
                let line_end = line_start + value_width;
                (cursor >= line_start && cursor <= line_end.min(char_count(value_text)))
                    .then(|| value_start_col as u16 + (cursor - line_start) as u16)
            });

            out.push(RenderedInspectorLine {
                row_index,
                line: Line::from(vec![
                    Span::styled(" ".repeat(max_name_len), name_style),
                    Span::styled(chunk.clone(), value_style),
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
        let value_chunks = wrap_text(value_text, value_width);

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
                let line_start = chunk_index * value_width;
                let line_end = line_start + value_width;
                (cursor >= line_start && cursor <= line_end.min(char_count(value_text)))
                    .then(|| prefix_width as u16 + (cursor - line_start) as u16)
            });

            out.push(RenderedInspectorLine {
                row_index,
                line: Line::from(vec![
                    Span::styled(prefix, name_style),
                    Span::styled(chunk.clone(), value_style),
                ]),
                cursor_col,
            });
        }

        out
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return vec![String::new()];
    }

    chars
        .chunks(width)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
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
    use crate::view::palette::Palette;

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
            &Palette::new(true),
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.row_index == 0));
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
            &Palette::new(true),
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
            &Palette::new(true),
        );
        assert!(lines.iter().any(|line| line.cursor_col.is_some()));
    }
}
