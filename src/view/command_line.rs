use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::view::palette::Palette;

pub fn widget(buffer: &str, palette: &Palette) -> Paragraph<'static> {
    Paragraph::new(Line::raw(format!(":{buffer}"))).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(palette.command_border),
    )
}
