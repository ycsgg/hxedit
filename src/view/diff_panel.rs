use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::util::format::{ascii_char, hex_pair};
use crate::view::hex_grid;
use crate::view::palette::Palette;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffPanelRow {
    pub display_offset: u64,
    pub cells: Vec<DiffPanelCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffPanelCell {
    pub other_byte: Option<u8>,
    pub kind: DiffPanelCellKind,
    pub other_offset: Option<u64>,
    pub current_display_offset: Option<u64>,
    pub visual_display_offset: Option<u64>,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffPanelCellKind {
    Equal,
    Replace,
    OnlyCurrent,
    OnlyOther,
    Gap,
}

pub(crate) fn build_lines(
    rows: &[DiffPanelRow],
    offset_width: usize,
    bytes_per_line: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| build_row(row, offset_width, bytes_per_line, palette))
        .collect()
}

pub(crate) fn header_line(
    offset_width: usize,
    bytes_per_line: usize,
    palette: &Palette,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(bytes_per_line.saturating_mul(2).saturating_add(4));
    spans.push(Span::styled(
        format!("{:1$} ", "", offset_width),
        palette.gutter,
    ));
    spans.extend(hex_grid::column_header_spans(bytes_per_line, palette));
    spans.push(Span::raw("  "));
    spans.push(Span::styled("ASCII", palette.gutter));
    Line::from(spans)
}

pub(crate) fn byte_col_from_x(x: u16, offset_width: usize, bytes_per_line: usize) -> Option<usize> {
    if bytes_per_line == 0 {
        return None;
    }
    let gutter_width = offset_width.saturating_add(1) as u16;
    if x < gutter_width {
        return Some(0);
    }

    let hex_x = x.saturating_sub(gutter_width);
    if let Some(col) = hex_col_from_x(hex_x, bytes_per_line) {
        return Some(col);
    }

    let ascii_start = gutter_width
        .saturating_add(hex_grid_width(bytes_per_line))
        .saturating_add(2);
    if x >= ascii_start {
        return ascii_col_from_x(x.saturating_sub(ascii_start), bytes_per_line);
    }
    None
}

fn hex_grid_width(bytes_per_line: usize) -> u16 {
    let mut width = 0_u16;
    for col in 0..bytes_per_line {
        width = width.saturating_add(2);
        if col + 1 != bytes_per_line {
            width = width.saturating_add(if bytes_per_line >= 8 && col + 1 == bytes_per_line / 2 {
                3
            } else {
                1
            });
        }
    }
    width
}

fn hex_col_from_x(x: u16, bytes_per_line: usize) -> Option<usize> {
    let mut cursor_x = 0_u16;
    for col in 0..bytes_per_line {
        let separator_width = if col + 1 == bytes_per_line {
            0
        } else if bytes_per_line >= 8 && col + 1 == bytes_per_line / 2 {
            3
        } else {
            1
        };
        let cell_width = 2 + separator_width;
        if x >= cursor_x && x < cursor_x + cell_width {
            return Some(col);
        }
        cursor_x += cell_width;
    }
    None
}

fn ascii_col_from_x(x: u16, bytes_per_line: usize) -> Option<usize> {
    let half = bytes_per_line / 2;
    if bytes_per_line >= 8 {
        if x < half as u16 {
            Some(x as usize)
        } else if x == half as u16 {
            Some(half)
        } else {
            let col = x as usize - 1;
            (col < bytes_per_line).then_some(col)
        }
    } else {
        let col = x as usize;
        (col < bytes_per_line).then_some(col)
    }
}

fn build_row(
    row: &DiffPanelRow,
    offset_width: usize,
    bytes_per_line: usize,
    palette: &Palette,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(bytes_per_line * 4 + 4);
    spans.push(Span::styled(
        format!("{:01$x} ", row.display_offset, offset_width),
        palette.gutter,
    ));

    for (idx, cell) in row.cells.iter().enumerate() {
        let mut style = cell_style(cell.kind, palette);
        if cell.active {
            style = palette.diff_active.patch(style);
        }
        match cell.other_byte {
            Some(byte) => {
                let pair = hex_pair(crate::core::document::ByteSlot::Present(byte));
                spans.push(Span::styled(pair[0].to_string(), style));
                spans.push(Span::styled(pair[1].to_string(), style));
            }
            None => {
                let placeholder = if cell.kind == DiffPanelCellKind::Gap {
                    "  "
                } else {
                    "__"
                };
                spans.push(Span::styled(placeholder, style));
            }
        }
        if idx + 1 != row.cells.len() {
            if bytes_per_line >= 8 && idx + 1 == bytes_per_line / 2 {
                spans.push(Span::styled(" │ ", palette.separator));
            } else {
                spans.push(Span::raw(" "));
            }
        }
    }

    spans.push(Span::raw("  "));
    for (idx, cell) in row.cells.iter().enumerate() {
        let mut style = cell_style(cell.kind, palette);
        if cell.active {
            style = palette.diff_active.patch(style);
        }
        let ch = cell
            .other_byte
            .map(|byte| ascii_char(crate::core::document::ByteSlot::Present(byte)))
            .unwrap_or(' ');
        spans.push(Span::styled(ch.to_string(), style));
        if bytes_per_line >= 8 && idx + 1 == bytes_per_line / 2 && idx + 1 != row.cells.len() {
            spans.push(Span::styled("│", palette.separator));
        }
    }

    Line::from(spans)
}

fn cell_style(kind: DiffPanelCellKind, palette: &Palette) -> Style {
    match kind {
        DiffPanelCellKind::Equal => palette.separator,
        DiffPanelCellKind::Replace => palette.diff_replace,
        DiffPanelCellKind::OnlyCurrent => palette.diff_only_current,
        DiffPanelCellKind::OnlyOther => palette.diff_only_other,
        DiffPanelCellKind::Gap => palette.separator,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;
    use crate::view::palette::ColorLevel;

    #[test]
    fn missing_current_bytes_render_as_red_placeholder() {
        let palette = Palette::new(ColorLevel::Basic);
        let lines = build_lines(
            &[DiffPanelRow {
                display_offset: 0,
                cells: vec![DiffPanelCell {
                    other_byte: None,
                    kind: DiffPanelCellKind::OnlyCurrent,
                    other_offset: None,
                    current_display_offset: Some(0),
                    visual_display_offset: Some(0),
                    active: false,
                }],
            }],
            1,
            1,
            &palette,
        );

        assert_eq!(lines[0].spans[1].content.as_ref(), "__");
        assert_eq!(lines[0].spans[1].style.bg, Some(Color::Red));
    }

    #[test]
    fn neutral_gaps_do_not_render_diff_placeholder() {
        let palette = Palette::new(ColorLevel::Basic);
        let lines = build_lines(
            &[DiffPanelRow {
                display_offset: 0,
                cells: vec![DiffPanelCell {
                    other_byte: None,
                    kind: DiffPanelCellKind::Gap,
                    other_offset: None,
                    current_display_offset: None,
                    visual_display_offset: None,
                    active: false,
                }],
            }],
            1,
            1,
            &palette,
        );

        assert_eq!(lines[0].spans[1].content.as_ref(), "  ");
        assert_eq!(lines[0].spans[1].style.bg, None);
    }

    #[test]
    fn byte_col_from_x_counts_gutter_hex_and_ascii_cells() {
        assert_eq!(byte_col_from_x(1, 8, 16), Some(0));
        assert_eq!(byte_col_from_x(8, 8, 16), Some(0));
        assert_eq!(byte_col_from_x(9, 8, 16), Some(0));
        assert_eq!(byte_col_from_x(11, 8, 16), Some(0));
        assert_eq!(byte_col_from_x(12, 8, 16), Some(1));
        assert_eq!(byte_col_from_x(35, 8, 16), Some(8));
        assert_eq!(byte_col_from_x(60, 8, 16), Some(0));
        assert_eq!(byte_col_from_x(69, 8, 16), Some(8));
    }
}
