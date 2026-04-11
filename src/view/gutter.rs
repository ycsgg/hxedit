use ratatui::text::Line;

use crate::util::format::format_offset;
use crate::view::palette::Palette;

pub fn build(offsets: &[u64], width: usize, palette: &Palette) -> Vec<Line<'static>> {
    offsets
        .iter()
        .map(|offset| Line::styled(format_offset(*offset, width), palette.gutter))
        .collect()
}
