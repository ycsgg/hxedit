use ratatui::text::{Line, Span};

use crate::util::format::format_offset;
use crate::view::palette::Palette;

pub fn build(offsets: &[u64], width: usize, palette: &Palette) -> Vec<Line<'static>> {
    offsets
        .iter()
        .map(|offset| Line::styled(format_offset(*offset, width), palette.gutter))
        .collect()
}

pub(crate) fn build_with_bookmarks(
    offsets: &[u64],
    width: usize,
    markers: &[BookmarkGutterMarker],
    palette: &Palette,
) -> Vec<Line<'static>> {
    offsets
        .iter()
        .enumerate()
        .map(|(row, offset)| {
            let marker = markers.get(row).copied().unwrap_or_default();
            let (label, style) = match marker {
                BookmarkGutterMarker::None => ("  ", palette.gutter),
                BookmarkGutterMarker::Bookmark(color) => (" ◆", palette.bookmark_marker(color)),
                BookmarkGutterMarker::Note(color) => (" ●", palette.bookmark_marker(color)),
            };
            Line::from(vec![
                Span::styled(format_offset(*offset, width), palette.gutter),
                Span::styled(label, style),
            ])
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum BookmarkGutterMarker {
    #[default]
    None,
    Bookmark(usize),
    Note(usize),
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::{build_with_bookmarks, BookmarkGutterMarker};
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn bookmark_marker_uses_entry_color() {
        let lines = build_with_bookmarks(
            &[0],
            8,
            &[BookmarkGutterMarker::Note(1)],
            &Palette::new(ColorLevel::Basic),
        );

        assert_eq!(lines[0].spans[1].content, " ●");
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Red));
    }
}
