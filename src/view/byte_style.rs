use ratatui::style::Style;

use crate::core::document::ByteSlot;
use crate::view::palette::Palette;

pub fn slot_style(slot: ByteSlot, palette: &Palette) -> Style {
    match slot {
        ByteSlot::Present(byte) => {
            if byte == 0 {
                palette.null
            } else if byte.is_ascii_graphic() {
                palette.printable
            } else if byte.is_ascii_whitespace() {
                palette.whitespace
            } else if byte.is_ascii() {
                palette.ascii_other
            } else {
                palette.non_ascii
            }
        }
        ByteSlot::Deleted => palette.deleted,
        ByteSlot::Empty => Style::default(),
    }
}
