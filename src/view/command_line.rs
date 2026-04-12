use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::commands::hints::CommandHint;
use crate::view::palette::Palette;

pub fn widget(buffer: &str, hint: CommandHint, palette: &Palette) -> Paragraph<'static> {
    Paragraph::new(vec![
        Line::raw(format!(":{buffer}")),
        Line::styled(hint.syntax, palette.command_hint),
        Line::styled(hint.details, palette.command_hint),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(palette.command_border),
    )
}
