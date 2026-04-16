use ratatui::text::{Line, Span};

use crate::app::StatusLevel;
use crate::mode::Mode;
use crate::view::palette::Palette;

pub(crate) struct StatusInfo<'a> {
    pub mode: Mode,
    pub path: &'a str,
    pub cursor: u64,
    pub display_len: u64,
    pub visible_len: u64,
    pub selection_span: Option<u64>,
    pub selection_logical_len: Option<u64>,
    pub paste_info: Option<&'a str>,
    pub dirty: bool,
    pub message: &'a str,
    pub message_level: StatusLevel,
    pub readonly: bool,
}

pub(crate) fn build(info: StatusInfo<'_>, palette: &Palette) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!(" {} ", info.mode.label()), palette.status),
        Span::raw(" "),
        Span::styled(info.path.to_owned(), palette.status),
        Span::raw("  "),
        Span::styled(format!("offset 0x{:x}", info.cursor), palette.status),
        Span::raw("  "),
        Span::styled(format!("len {}", info.display_len), palette.status),
        Span::raw("  "),
        Span::styled(format!("vis {}", info.visible_len), palette.status),
    ];

    if let Some(selection_span) = info.selection_span {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("sel(span) {}", selection_span),
            palette.status,
        ));
    }

    if let Some(selection_logical_len) = info.selection_logical_len {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("sel(logical) {}", selection_logical_len),
            palette.status,
        ));
    }

    if let Some(paste_info) = info.paste_info {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(paste_info.to_owned(), palette.status));
    }

    if info.readonly {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("[RO]", palette.status));
    }
    if info.dirty {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("[+]", palette.dirty));
    }
    if !info.message.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            info.message.to_owned(),
            match info.message_level {
                StatusLevel::Info => palette.status,
                StatusLevel::Warning => palette.warning,
                StatusLevel::Error => palette.error,
            },
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::{build, StatusInfo};
    use crate::app::StatusLevel;
    use crate::mode::{Mode, NibblePhase};
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn warning_messages_use_warning_style() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                mode: Mode::EditHex {
                    phase: NibblePhase::High,
                },
                path: "sample.bin",
                cursor: 0,
                display_len: 1,
                visible_len: 1,
                selection_span: None,
                selection_logical_len: None,
                paste_info: None,
                dirty: false,
                message: "png edit may break crc",
                message_level: StatusLevel::Warning,
                readonly: false,
            },
            &palette,
        );

        let warning_span = line.spans.last().expect("message span");
        assert_eq!(warning_span.style.fg, Some(Color::Black));
        assert_eq!(warning_span.style.bg, Some(Color::Yellow));
        assert!(warning_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn error_messages_use_error_style() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                mode: Mode::EditHex {
                    phase: NibblePhase::High,
                },
                path: "sample.bin",
                cursor: 0,
                display_len: 1,
                visible_len: 1,
                selection_span: None,
                selection_logical_len: None,
                paste_info: None,
                dirty: false,
                message: "document is read-only",
                message_level: StatusLevel::Error,
                readonly: false,
            },
            &palette,
        );

        let error_span = line.spans.last().expect("message span");
        assert_eq!(error_span.style.fg, Some(Color::White));
        assert_eq!(error_span.style.bg, Some(Color::Red));
        assert!(error_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn status_line_includes_visible_and_logical_selection_lengths() {
        let palette = Palette::new(ColorLevel::NoColor);
        let line = build(
            StatusInfo {
                mode: Mode::Normal,
                path: "sample.bin",
                cursor: 0x10,
                display_len: 12,
                visible_len: 10,
                selection_span: Some(4),
                selection_logical_len: Some(3),
                paste_info: None,
                dirty: false,
                message: "",
                message_level: StatusLevel::Info,
                readonly: false,
            },
            &palette,
        );

        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("len 12"));
        assert!(text.contains("vis 10"));
        assert!(text.contains("sel(span) 4"));
        assert!(text.contains("sel(logical) 3"));
    }
}
