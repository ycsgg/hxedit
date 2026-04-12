use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::PasteSource;
use crate::error::{HxError, HxResult};
use crate::view::palette::Palette;

pub(crate) fn separator_widget(height: u16, palette: &Palette) -> Paragraph<'static> {
    let lines = (0..height)
        .map(|_| Line::styled("│", palette.separator))
        .collect::<Vec<_>>();
    Paragraph::new(lines)
}

pub(crate) fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub(crate) fn parse_paste_payload(text: &str) -> HxResult<(Vec<u8>, PasteSource)> {
    if let Ok(hex) = crate::util::parse::parse_hex_stream(text) {
        return Ok((hex, PasteSource::Hex));
    }
    if let Ok(base64) = crate::util::parse::decode_base64(text) {
        return Ok((base64, PasteSource::Base64));
    }
    Err(HxError::InvalidPasteData(text.trim().to_owned()))
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
