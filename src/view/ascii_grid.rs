use ratatui::text::{Line, Span};

use crate::core::document::ByteSlot;
use crate::mode::Mode;
use crate::util::format::ascii_char;
use crate::view::byte_style::slot_style;
use crate::view::hex_grid::BookmarkOverlay;
use crate::view::palette::Palette;

#[derive(Debug, Clone, Copy, Default)]
pub struct AsciiGridOverlays<'a> {
    pub selection: Option<(u64, u64)>,
    pub bookmarks: &'a [BookmarkOverlay],
}

pub fn build(
    rows: &[Vec<ByteSlot>],
    row_offsets: &[u64],
    cursor: u64,
    _mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
    overlays: AsciiGridOverlays<'_>,
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
    build_projected(&projected, cursor, _mode, palette, bytes_per_line, overlays)
}

pub fn build_projected(
    rows: &[Vec<(ByteSlot, Option<u64>, bool)>],
    cursor: u64,
    _mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
    overlays: AsciiGridOverlays<'_>,
) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| {
            let mut spans = Vec::with_capacity(bytes_per_line + 2);
            for (col_idx, (slot, display_offset, only_other)) in row.iter().enumerate() {
                let mut style = slot_style(*slot, palette);
                if let Some(offset) = *display_offset {
                    if let Some(color) = bookmark_at(overlays.bookmarks, offset) {
                        style = palette.bookmark_highlight(color).patch(style);
                    }
                    if selected(overlays.selection, offset) {
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

fn bookmark_at(bookmarks: &[BookmarkOverlay], offset: u64) -> Option<usize> {
    bookmarks
        .iter()
        .find(|bookmark| offset >= bookmark.start && offset <= bookmark.end)
        .map(|bookmark| bookmark.color_index)
}

fn selected(selection: Option<(u64, u64)>, offset: u64) -> bool {
    selection
        .map(|(start, end)| offset >= start && offset <= end)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::{build, AsciiGridOverlays};
    use crate::core::document::ByteSlot;
    use crate::mode::Mode;
    use crate::view::hex_grid::BookmarkOverlay;
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn bookmark_overlay_underlines_ascii_with_requested_color() {
        let lines = build(
            &[vec![ByteSlot::Present(b'A')]],
            &[0],
            99,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            1,
            AsciiGridOverlays {
                selection: None,
                bookmarks: &[BookmarkOverlay {
                    start: 0,
                    end: 0,
                    color_index: 4,
                }],
            },
        );

        assert_eq!(lines[0].spans[0].style.underline_color, Some(Color::Blue));
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }
}
