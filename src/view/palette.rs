use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct Palette {
    pub gutter: Style,
    pub separator: Style,
    pub status: Style,
    pub dirty: Style,
    pub selection: Style,
    pub deleted: Style,
    pub null: Style,
    pub printable: Style,
    pub whitespace: Style,
    pub ascii_other: Style,
    pub non_ascii: Style,
    pub cursor: Style,
    pub cursor_nibble: Style,
    pub command_border: Style,
    pub command_hint: Style,
}

impl Palette {
    pub fn new(color: bool) -> Self {
        if color {
            Self {
                gutter: Style::default().fg(Color::DarkGray),
                separator: Style::default().fg(Color::DarkGray),
                status: Style::default().fg(Color::White),
                dirty: Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                selection: Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
                deleted: Style::default().fg(Color::Red),
                null: Style::default().fg(Color::DarkGray),
                printable: Style::default().fg(Color::Cyan),
                whitespace: Style::default().fg(Color::Green),
                ascii_other: Style::default().fg(Color::Green),
                non_ascii: Style::default().fg(Color::Yellow),
                cursor: Style::default()
                    .bg(Color::Blue)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                cursor_nibble: Style::default()
                    .bg(Color::LightBlue)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                command_border: Style::default().fg(Color::Cyan),
                command_hint: Style::default().fg(Color::DarkGray),
            }
        } else {
            let base = Style::default();
            Self {
                gutter: base,
                separator: base,
                status: base,
                dirty: base.add_modifier(Modifier::BOLD),
                selection: base.add_modifier(Modifier::REVERSED),
                deleted: base,
                null: base,
                printable: base,
                whitespace: base,
                ascii_other: base,
                non_ascii: base,
                cursor: base.add_modifier(Modifier::REVERSED),
                cursor_nibble: base.add_modifier(Modifier::REVERSED),
                command_border: base,
                command_hint: base,
            }
        }
    }
}
