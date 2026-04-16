use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::PasteSource;
use crate::view::palette::Palette;

pub(crate) fn separator_widget(height: u16, palette: &Palette) -> Paragraph<'static> {
    let lines = (0..height)
        .map(|_| Line::styled("│", palette.separator))
        .collect::<Vec<_>>();
    Paragraph::new(lines)
}

pub(crate) fn paste_summary(
    source: PasteSource,
    bytes: usize,
    preview: bool,
    data: &[u8],
) -> String {
    let head = data
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let suffix = if data.len() > 4 { " …" } else { "" };
    if preview {
        format!("preview {}:{} [{}{}]", source.label(), bytes, head, suffix)
    } else {
        format!("paste {}:{} [{}{}]", source.label(), bytes, head, suffix)
    }
}
