use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::core::document::ByteSlot;
use crate::mode::{Mode, NibblePhase};
use crate::util::format::hex_pair;
use crate::view::byte_style::slot_style;
use crate::view::palette::Palette;

#[derive(Debug, Clone, Copy, Default)]
pub struct HexGridOverlays {
    pub selection: Option<(u64, u64)>,
    pub inspector_highlight: Option<(u64, u64)>,
}

pub fn build(
    rows: &[Vec<ByteSlot>],
    row_offsets: &[u64],
    cursor: u64,
    mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
    overlays: HexGridOverlays,
) -> Vec<Line<'static>> {
    rows.iter()
        .enumerate()
        .map(|(row_idx, row)| {
            let mut spans = Vec::with_capacity(bytes_per_line * 4);
            for (col_idx, slot) in row.iter().enumerate() {
                let offset = row_offsets[row_idx] + col_idx as u64;
                let mut base = slot_style(*slot, palette);
                if highlighted(overlays.inspector_highlight, offset) {
                    base = palette.inspector_highlight.patch(base);
                }
                if selected(overlays.selection, offset) {
                    base = palette.selection.patch(base);
                }
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

fn selected(selection: Option<(u64, u64)>, offset: u64) -> bool {
    selection
        .map(|(start, end)| offset >= start && offset <= end)
        .unwrap_or(false)
}

fn highlighted(highlight: Option<(u64, u64)>, offset: u64) -> bool {
    highlight
        .map(|(start, end)| offset >= start && offset <= end)
        .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::{build, HexGridOverlays};
    use crate::core::document::ByteSlot;
    use crate::mode::Mode;
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn inspector_highlight_underlines_selected_field_bytes() {
        let lines = build(
            &[vec![ByteSlot::Present(0x41), ByteSlot::Present(0x42)]],
            &[0],
            99,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            2,
            HexGridOverlays {
                selection: None,
                inspector_highlight: Some((1, 1)),
            },
        );

        let line = &lines[0];
        assert!(!line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[3]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn cursor_keeps_field_highlight_modifier() {
        let lines = build(
            &[vec![ByteSlot::Present(0x41)]],
            &[0],
            0,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            1,
            HexGridOverlays {
                selection: None,
                inspector_highlight: Some((0, 0)),
            },
        );

        let line = &lines[0];
        assert!(line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[1]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }
}
