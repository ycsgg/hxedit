use ratatui::text::{Line, Span};

use crate::core::document::ByteSlot;
use crate::mode::Mode;
use crate::util::format::ascii_char;
use crate::view::byte_style::slot_style;
use crate::view::palette::Palette;

pub fn build(
    rows: &[Vec<ByteSlot>],
    row_offsets: &[u64],
    cursor: u64,
    _mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
    selection: Option<(u64, u64)>,
) -> Vec<Line<'static>> {
    rows.iter()
        .enumerate()
        .map(|(row_idx, row)| {
            let mut spans = Vec::with_capacity(bytes_per_line + 2);
            for (col_idx, slot) in row.iter().enumerate() {
                let offset = row_offsets[row_idx] + col_idx as u64;
                let mut style = slot_style(*slot, palette);
                if selected(selection, offset) {
                    style = palette.selection.patch(style);
                }
                if offset == cursor {
                    style = palette.cursor.patch(style);
                }
                spans.push(Span::styled(ascii_char(*slot).to_string(), style));
                if bytes_per_line >= 8
                    && col_idx + 1 == bytes_per_line / 2
                    && col_idx + 1 != row.len()
                {
                    spans.push(Span::styled("│", palette.separator));
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn selected(selection: Option<(u64, u64)>, offset: u64) -> bool {
    selection
        .map(|(start, end)| offset >= start && offset <= end)
        .unwrap_or(false)
}
