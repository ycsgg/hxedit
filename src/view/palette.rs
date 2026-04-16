use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct Palette {
    pub gutter: Style,
    pub separator: Style,
    pub status: Style,
    pub warning: Style,
    pub error: Style,
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
    // ── Inspector styles ──
    /// Inspector structure name header.
    pub inspector_header: Style,
    /// Inspector normal field name.
    pub inspector_field: Style,
    /// Inspector field value.
    pub inspector_value: Style,
    /// Inspector currently selected / associated field (highlight).
    pub inspector_active: Style,
    /// Inspector field being edited.
    pub inspector_edit: Style,
}

impl Palette {
    pub fn new(color: bool) -> Self {
        if color {
            Self {
                gutter: Style::default().fg(Color::DarkGray),
                separator: Style::default().fg(Color::DarkGray),
                status: Style::default().fg(Color::White),
                warning: Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                error: Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
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
                inspector_header: Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
                inspector_field: Style::default().fg(Color::Gray),
                inspector_value: Style::default().fg(Color::Cyan),
                inspector_active: Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
                inspector_edit: Style::default().bg(Color::Yellow).fg(Color::Black),
            }
        } else {
            let base = Style::default();
            Self {
                gutter: base,
                separator: base,
                status: base,
                warning: base
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED),
                error: base
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED),
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
                inspector_header: base.add_modifier(Modifier::BOLD),
                inspector_field: base,
                inspector_value: base,
                inspector_active: base.add_modifier(Modifier::REVERSED),
                inspector_edit: base.add_modifier(Modifier::UNDERLINED),
            }
        }
    }
}
