use ratatui::text::{Line, Span};

use crate::mode::Mode;
use crate::view::palette::Palette;

pub struct StatusInfo<'a> {
    pub mode: Mode,
    pub path: &'a str,
    pub cursor: u64,
    pub len: u64,
    pub selection_len: Option<u64>,
    pub paste_info: Option<&'a str>,
    pub dirty: bool,
    pub message: &'a str,
    pub readonly: bool,
}

pub fn build(info: StatusInfo<'_>, palette: &Palette) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!(" {} ", info.mode.label()), palette.status),
        Span::raw(" "),
        Span::styled(info.path.to_owned(), palette.status),
        Span::raw("  "),
        Span::styled(format!("offset 0x{:x}", info.cursor), palette.status),
        Span::raw("  "),
        Span::styled(format!("len {}", info.len), palette.status),
    ];

    if let Some(selection_len) = info.selection_len {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("sel {}", selection_len),
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
            if is_warning_message(info.message) {
                palette.warning
            } else {
                palette.status
            },
        ));
    }
    Line::from(spans)
}

fn is_warning_message(message: &str) -> bool {
    message.contains("warning:")
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::{build, StatusInfo};
    use crate::mode::{Mode, NibblePhase};
    use crate::view::palette::Palette;

    #[test]
    fn warning_messages_use_warning_style() {
        let palette = Palette::new(true);
        let line = build(
            StatusInfo {
                mode: Mode::EditHex {
                    phase: NibblePhase::High,
                },
                path: "sample.bin",
                cursor: 0,
                len: 1,
                selection_len: None,
                paste_info: None,
                dirty: false,
                message: "warning: png edit may break crc",
                readonly: false,
            },
            &palette,
        );

        let warning_span = line.spans.last().expect("message span");
        assert_eq!(warning_span.style.fg, Some(Color::Black));
        assert_eq!(warning_span.style.bg, Some(Color::Yellow));
        assert!(warning_span.style.add_modifier.contains(Modifier::BOLD));
    }
}
