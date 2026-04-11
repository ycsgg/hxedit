use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::core::document::ByteSlot;
use crate::mode::{Mode, NibblePhase};
use crate::util::format::hex_pair;
use crate::view::byte_style::slot_style;
use crate::view::palette::Palette;

pub fn build(
    rows: &[Vec<ByteSlot>],
    row_offsets: &[u64],
    cursor: u64,
    mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
) -> Vec<Line<'static>> {
    rows.iter()
        .enumerate()
        .map(|(row_idx, row)| {
            let mut spans = Vec::with_capacity(bytes_per_line * 4);
            for (col_idx, slot) in row.iter().enumerate() {
                let offset = row_offsets[row_idx] + col_idx as u64;
                let base = slot_style(*slot, palette);
                let pair = hex_pair(*slot);
                let is_cursor = offset == cursor;
                let phase = match mode {
                    Mode::EditHex { phase } if is_cursor => Some(phase),
                    _ => None,
                };

                spans.push(Span::styled(
                    pair[0].to_string(),
                    style_for_nibble(base, is_cursor, phase, true, palette),
                ));
                spans.push(Span::styled(
                    pair[1].to_string(),
                    style_for_nibble(base, is_cursor, phase, false, palette),
                ));

                if col_idx + 1 != row.len() {
                    if bytes_per_line >= 8 && col_idx + 1 == bytes_per_line / 2 {
                        spans.push(Span::styled(" │ ", palette.separator));
                    } else {
                        spans.push(Span::raw(" "));
                    }
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn style_for_nibble(
    base: Style,
    is_cursor: bool,
    phase: Option<NibblePhase>,
    is_high: bool,
    palette: &Palette,
) -> Style {
    if !is_cursor {
        return base;
    }
    match phase {
        Some(NibblePhase::High) if is_high => palette.cursor_nibble.patch(base),
        Some(NibblePhase::Low) if !is_high => palette.cursor_nibble.patch(base),
        Some(_) => palette.cursor.patch(base),
        None => palette.cursor.patch(base),
    }
}
