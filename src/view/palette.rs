use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorLevel {
    NoColor,
    Basic,
    Extended,
    TrueColor,
}

impl ColorLevel {
    pub fn detect(no_color_flag: bool) -> Self {
        if no_color_flag {
            return Self::NoColor;
        }
        if std::env::var("NO_COLOR").is_ok() {
            return Self::NoColor;
        }
        if Self::is_truecolor_terminal() {
            return Self::TrueColor;
        }
        match supports_color::on(supports_color::Stream::Stdout) {
            Some(support) if support.has_16m => Self::TrueColor,
            Some(support) if support.has_256 => Self::Extended,
            Some(support) if support.has_basic => Self::Basic,
            _ => Self::NoColor,
        }
    }

    fn is_truecolor_terminal() -> bool {
        if std::env::var("WT_SESSION").is_ok() {
            return true;
        }
        match std::env::var("COLORTERM").as_deref() {
            Ok("truecolor") | Ok("24bit") => return true,
            _ => {}
        }
        if std::env::var("TERM_PROGRAM").as_deref() == Ok("vscode") {
            return true;
        }
        false
    }
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub gutter: Style,
    pub separator: Style,
    pub status: Style,
    pub notice: Style,
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
    pub inspector_header: Style,
    pub inspector_field: Style,
    pub inspector_value: Style,
    pub inspector_active: Style,
    pub inspector_edit: Style,
    pub inspector_highlight: Style,
}

impl Palette {
    pub fn new(level: ColorLevel) -> Self {
        match level {
            ColorLevel::TrueColor => Self::truecolor(),
            ColorLevel::Extended => Self::extended(),
            ColorLevel::Basic => Self::basic(),
            ColorLevel::NoColor => Self::no_color(),
        }
    }

    fn truecolor() -> Self {
        Self {
            gutter: Style::default().fg(Color::Rgb(80, 80, 80)),
            separator: Style::default().fg(Color::Rgb(80, 80, 80)),
            status: Style::default().fg(Color::Rgb(220, 220, 220)),
            notice: Style::default()
                .fg(Color::Rgb(120, 200, 255))
                .add_modifier(Modifier::BOLD),
            warning: Style::default()
                .bg(Color::Rgb(220, 180, 0))
                .fg(Color::Rgb(30, 30, 30))
                .add_modifier(Modifier::BOLD),
            error: Style::default()
                .bg(Color::Rgb(200, 50, 50))
                .fg(Color::Rgb(255, 255, 255))
                .add_modifier(Modifier::BOLD),
            dirty: Style::default()
                .fg(Color::Rgb(220, 180, 0))
                .add_modifier(Modifier::BOLD),
            selection: Style::default()
                .bg(Color::Rgb(40, 80, 160))
                .fg(Color::Rgb(240, 240, 240))
                .add_modifier(Modifier::BOLD),
            deleted: Style::default().fg(Color::Rgb(200, 60, 60)),
            null: Style::default().fg(Color::Rgb(100, 100, 100)),
            printable: Style::default().fg(Color::Rgb(80, 200, 200)),
            whitespace: Style::default().fg(Color::Rgb(80, 180, 80)),
            ascii_other: Style::default().fg(Color::Rgb(80, 180, 80)),
            non_ascii: Style::default().fg(Color::Rgb(220, 180, 0)),
            cursor: Style::default()
                .bg(Color::Rgb(50, 100, 200))
                .fg(Color::Rgb(255, 255, 255))
                .add_modifier(Modifier::BOLD),
            cursor_nibble: Style::default()
                .bg(Color::Rgb(220, 220, 220))
                .fg(Color::Rgb(30, 30, 30))
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            command_border: Style::default().fg(Color::Rgb(80, 200, 200)),
            command_hint: Style::default().fg(Color::Rgb(118, 118, 118)),
            inspector_header: Style::default()
                .fg(Color::Rgb(200, 120, 200))
                .add_modifier(Modifier::BOLD),
            inspector_field: Style::default().fg(Color::Rgb(160, 160, 160)),
            inspector_value: Style::default().fg(Color::Rgb(80, 200, 200)),
            inspector_active: Style::default()
                .bg(Color::Rgb(80, 80, 80))
                .fg(Color::Rgb(240, 240, 240))
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            inspector_edit: Style::default()
                .bg(Color::Rgb(130, 180, 230))
                .fg(Color::Rgb(30, 30, 30)),
            inspector_highlight: Style::default()
                .fg(Color::Rgb(255, 220, 120))
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD),
        }
    }

    fn extended() -> Self {
        Self {
            gutter: Style::default().fg(Color::Indexed(245)),
            separator: Style::default().fg(Color::Indexed(245)),
            status: Style::default().fg(Color::Indexed(252)),
            notice: Style::default()
                .fg(Color::Indexed(117))
                .add_modifier(Modifier::BOLD),
            warning: Style::default()
                .bg(Color::Indexed(220))
                .fg(Color::Indexed(235))
                .add_modifier(Modifier::BOLD),
            error: Style::default()
                .bg(Color::Indexed(160))
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
            dirty: Style::default()
                .fg(Color::Indexed(220))
                .add_modifier(Modifier::BOLD),
            selection: Style::default()
                .bg(Color::Indexed(25))
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
            deleted: Style::default().fg(Color::Indexed(160)),
            null: Style::default().fg(Color::Indexed(245)),
            printable: Style::default().fg(Color::Indexed(123)),
            whitespace: Style::default().fg(Color::Indexed(114)),
            ascii_other: Style::default().fg(Color::Indexed(114)),
            non_ascii: Style::default().fg(Color::Indexed(220)),
            cursor: Style::default()
                .bg(Color::Indexed(25))
                .fg(Color::Indexed(235))
                .add_modifier(Modifier::BOLD),
            cursor_nibble: Style::default()
                .bg(Color::Indexed(252))
                .fg(Color::Indexed(235))
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            command_border: Style::default().fg(Color::Indexed(123)),
            command_hint: Style::default().fg(Color::Indexed(245)),
            inspector_header: Style::default()
                .fg(Color::Indexed(183))
                .add_modifier(Modifier::BOLD),
            inspector_field: Style::default().fg(Color::Indexed(252)),
            inspector_value: Style::default().fg(Color::Indexed(123)),
            inspector_active: Style::default()
                .bg(Color::Indexed(240))
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            inspector_edit: Style::default()
                .bg(Color::Indexed(153))
                .fg(Color::Indexed(235)),
            inspector_highlight: Style::default()
                .fg(Color::Indexed(222))
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD),
        }
    }

    fn basic() -> Self {
        Self {
            gutter: Style::default().fg(Color::DarkGray),
            separator: Style::default().fg(Color::DarkGray),
            status: Style::default().fg(Color::White),
            notice: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
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
                .bg(Color::White)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
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
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            inspector_edit: Style::default().bg(Color::LightBlue).fg(Color::Black),
            inspector_highlight: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD),
        }
    }

    fn no_color() -> Self {
        let base = Style::default();
        Self {
            gutter: base,
            separator: base,
            status: base,
            notice: base
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
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
            inspector_highlight: base
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorLevel, Palette};
    use ratatui::style::Color;

    #[test]
    fn no_color_flag_overrides_detection() {
        let level = ColorLevel::detect(true);
        assert_eq!(level, ColorLevel::NoColor);
    }

    #[test]
    fn detect_returns_truecolor_in_windows_terminal() {
        let in_wt = std::env::var("WT_SESSION").is_ok();
        let no_color = std::env::var("NO_COLOR").is_ok();
        let level = ColorLevel::detect(false);
        if in_wt && !no_color {
            assert_eq!(level, ColorLevel::TrueColor);
        }
    }

    #[test]
    fn detect_returns_truecolor_with_colorterm_truecolor() {
        let colorterm_is_truecolor = matches!(
            std::env::var("COLORTERM").as_deref(),
            Ok("truecolor") | Ok("24bit")
        );
        let no_color = std::env::var("NO_COLOR").is_ok();
        let level = ColorLevel::detect(false);
        if colorterm_is_truecolor && !no_color {
            assert_eq!(level, ColorLevel::TrueColor);
        }
    }

    #[test]
    fn truecolor_palette_uses_rgb_colors() {
        let palette = Palette::new(ColorLevel::TrueColor);
        assert!(matches!(palette.gutter.fg, Some(Color::Rgb(_, _, _))));
        assert!(matches!(palette.cursor.bg, Some(Color::Rgb(_, _, _))));
        assert!(matches!(palette.warning.bg, Some(Color::Rgb(_, _, _))));
    }

    #[test]
    fn extended_palette_uses_indexed_colors() {
        let palette = Palette::new(ColorLevel::Extended);
        assert!(matches!(palette.gutter.fg, Some(Color::Indexed(_))));
        assert!(matches!(palette.cursor.bg, Some(Color::Indexed(_))));
        assert!(matches!(palette.warning.bg, Some(Color::Indexed(_))));
    }

    #[test]
    fn basic_palette_uses_named_colors() {
        let palette = Palette::new(ColorLevel::Basic);
        assert!(matches!(palette.gutter.fg, Some(Color::DarkGray)));
        assert!(matches!(palette.cursor.bg, Some(Color::Blue)));
    }

    #[test]
    fn no_color_palette_has_no_fg_bg() {
        let palette = Palette::new(ColorLevel::NoColor);
        assert!(palette.gutter.fg.is_none());
        assert!(palette.gutter.bg.is_none());
        assert!(palette.printable.fg.is_none());
        assert!(palette.printable.bg.is_none());
    }
}
