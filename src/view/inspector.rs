use ratatui::text::{Line, Span};

use crate::format::parse::InspectorRow;
use crate::view::palette::Palette;

pub const FIELD_NAME_MIN_WIDTH: u16 = 16;

#[derive(Debug, Clone)]
pub struct RenderedInspectorLine {
    pub row_index: usize,
    pub line: Line<'static>,
    pub cursor_col: Option<u16>,
}

pub fn build_wrapped(
    state_rows: &[InspectorRow],
    selected_row: usize,
    editing: Option<(&str, usize)>,
    width: u16,
    palette: &Palette,
) -> Vec<RenderedInspectorLine> {
    let width = width.max(1) as usize;
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
                    width,
                    cursor_pos,
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
    width: usize,
    cursor_pos: Option<usize>,
) -> Vec<RenderedInspectorLine> {
    let desired_prefix = char_count(name_str).max(FIELD_NAME_MIN_WIDTH as usize);
    let prefix_width = desired_prefix.min(width.saturating_sub(1).max(1));
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
