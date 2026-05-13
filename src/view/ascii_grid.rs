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
    let projected = rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| {
            row.iter()
                .enumerate()
                .map(|(col_idx, slot)| (*slot, Some(row_offsets[row_idx] + col_idx as u64), false))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    build_projected(
        &projected,
        cursor,
        _mode,
        palette,
        bytes_per_line,
        selection,
    )
}

pub fn build_projected(
    rows: &[Vec<(ByteSlot, Option<u64>, bool)>],
    cursor: u64,
    _mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
    selection: Option<(u64, u64)>,
) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| {
            let mut spans = Vec::with_capacity(bytes_per_line + 2);
            for (col_idx, (slot, display_offset, only_other)) in row.iter().enumerate() {
                let mut style = slot_style(*slot, palette);
                if let Some(offset) = *display_offset {
                    if selected(selection, offset) {
                        style = palette.selection.patch(style);
                    }
                    if offset == cursor {
                        style = palette.cursor.patch(style);
                    }
                }
                let ch = if *only_other { ' ' } else { ascii_char(*slot) };
                spans.push(Span::styled(ch.to_string(), style));
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
