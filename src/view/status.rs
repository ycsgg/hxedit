use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::StatusLevel;
use crate::mode::Mode;
use crate::view::palette::Palette;

pub(crate) struct StatusInfo<'a> {
    pub main_view_label: Option<&'a str>,
    pub mode: Mode,
    pub path: &'a str,
    pub cursor: u64,
    pub cursor_label: Option<String>,
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

pub(crate) fn build(info: StatusInfo<'_>, width: u16, palette: &Palette) -> Line<'static> {
    let mut items = status_items(info, palette);
    fit_items(&mut items, width as usize);
    line_from_items(items)
}

#[derive(Clone)]
struct StatusItem {
    text: String,
    style: Style,
    drop_priority: u8,
    truncatable: bool,
}

fn status_items(info: StatusInfo<'_>, palette: &Palette) -> Vec<StatusItem> {
    let message_style = match info.message_level {
        StatusLevel::Info => palette.status,
        StatusLevel::Notice => palette.notice,
        StatusLevel::Warning => palette.warning,
        StatusLevel::Error => palette.error,
    };
    let urgent_message = matches!(
        info.message_level,
        StatusLevel::Warning | StatusLevel::Error
    ) && !info.message.is_empty();
    let cursor_label = info
        .cursor_label
        .clone()
        .unwrap_or_else(|| format!("offset 0x{:x}", info.cursor));

    let mut items = vec![StatusItem::required(
        format!(" {} ", info.mode.label()),
        palette.status,
        false,
    )];

    if urgent_message {
        items.push(StatusItem::required(
            info.message.to_owned(),
            message_style,
            true,
        ));
        push_core_status_items(&mut items, &info, cursor_label, palette);
        push_context_status_items(&mut items, &info, palette, false);
    } else {
        items.push(StatusItem::optional(
            info.path.to_owned(),
            palette.status,
            4,
            true,
        ));
        push_core_status_items(&mut items, &info, cursor_label, palette);
        push_context_status_items(&mut items, &info, palette, true);
        if !info.message.is_empty() {
            items.push(StatusItem::required(
                info.message.to_owned(),
                message_style,
                true,
            ));
        }
    }
    items
}

fn push_core_status_items(
    items: &mut Vec<StatusItem>,
    info: &StatusInfo<'_>,
    cursor_label: String,
    palette: &Palette,
) {
    items.push(StatusItem::optional(cursor_label, palette.status, 1, false));
    if let Some(main_view_label) = info.main_view_label {
        items.push(StatusItem::optional(
            format!("view {}", main_view_label),
            palette.status,
            1,
            false,
        ));
    }
    if info.readonly {
        items.push(StatusItem::optional(
            "[RO]".to_owned(),
            palette.status,
            1,
            false,
        ));
    }
    if info.dirty {
        items.push(StatusItem::optional(
            "[+]".to_owned(),
            palette.dirty,
            1,
            false,
        ));
    }
}

fn push_context_status_items(
    items: &mut Vec<StatusItem>,
    info: &StatusInfo<'_>,
    palette: &Palette,
    include_path_metrics: bool,
) {
    if include_path_metrics {
        items.push(StatusItem::optional(
            format!("len {}", info.display_len),
            palette.status,
            3,
            false,
        ));
        items.push(StatusItem::optional(
            format!("vis {}", info.visible_len),
            palette.status,
            3,
            false,
        ));
    }

    if let Some(selection_span) = info.selection_span {
        items.push(StatusItem::optional(
            format!("sel(span) {}", selection_span),
            palette.status,
            5,
            false,
        ));
    }

    if let Some(selection_logical_len) = info.selection_logical_len {
        items.push(StatusItem::optional(
            format!("sel(logical) {}", selection_logical_len),
            palette.status,
            5,
            false,
        ));
    }

    if let Some(paste_info) = info.paste_info {
        items.push(StatusItem::optional(
            paste_info.to_owned(),
            palette.status,
            5,
            true,
        ));
    }

    if !include_path_metrics {
        items.push(StatusItem::optional(
            info.path.to_owned(),
            palette.status,
            4,
            true,
        ));
        items.push(StatusItem::optional(
            format!("len {}", info.display_len),
            palette.status,
            5,
            false,
        ));
        items.push(StatusItem::optional(
            format!("vis {}", info.visible_len),
            palette.status,
            5,
            false,
        ));
    }
}

impl StatusItem {
    fn required(text: String, style: Style, truncatable: bool) -> Self {
        Self {
            text,
            style,
            drop_priority: 0,
            truncatable,
        }
    }

    fn optional(text: String, style: Style, drop_priority: u8, truncatable: bool) -> Self {
        Self {
            text,
            style,
            drop_priority,
            truncatable,
        }
    }
}

fn fit_items(items: &mut Vec<StatusItem>, width: usize) {
    if width == 0 {
        items.clear();
        return;
    }

    while items_width(items) > width {
        let Some(priority) = items.iter().map(|item| item.drop_priority).max() else {
            return;
        };
        if priority == 0 {
            break;
        }
        if let Some(index) = items
            .iter()
            .rposition(|item| item.drop_priority == priority)
        {
            items.remove(index);
        } else {
            break;
        }
    }

    while items_width(items) > width {
        if truncate_one_item(items, width) {
            continue;
        }
        if items.pop().is_none() {
            break;
        }
    }
}

fn truncate_one_item(items: &mut Vec<StatusItem>, width: usize) -> bool {
    let Some(index) = items
        .iter()
        .rposition(|item| item.truncatable && !item.text.is_empty())
    else {
        return false;
    };
    let others_width = items_width_around_retained_item(items, index);
    if others_width >= width {
        items.remove(index);
        return true;
    }
    let allowed = width - others_width;
    let truncated = truncate_text(&items[index].text, allowed);
    if truncated == items[index].text {
        items.remove(index);
    } else {
        items[index].text = truncated;
    }
    true
}

fn line_from_items(items: Vec<StatusItem>) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(item.text, item.style));
    }
    Line::from(spans)
}

fn items_width(items: &[StatusItem]) -> usize {
    items_width_excluding(items, usize::MAX)
}

fn items_width_excluding(items: &[StatusItem], excluded_index: usize) -> usize {
    let included = items
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != excluded_index)
        .collect::<Vec<_>>();
    let item_width = included
        .iter()
        .map(|(_, item)| text_width(&item.text))
        .sum::<usize>();
    let separator_width = included.len().saturating_sub(1) * 2;
    item_width + separator_width
}

fn items_width_around_retained_item(items: &[StatusItem], retained_index: usize) -> usize {
    let item_width = items
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != retained_index)
        .map(|(_, item)| text_width(&item.text))
        .sum::<usize>();
    let separator_width = items.len().saturating_sub(1) * 2;
    item_width + separator_width
}

fn text_width(text: &str) -> usize {
    text.chars().count()
}

fn truncate_text(text: &str, width: usize) -> String {
    if text_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut truncated = text.chars().take(width - 1).collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};
    use ratatui::text::Line;

    use super::{build, text_width, StatusInfo};
    use crate::app::StatusLevel;
    use crate::mode::{Mode, NibblePhase};
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn warning_messages_use_warning_style() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                main_view_label: None,
                mode: Mode::EditHex {
                    phase: NibblePhase::High,
                },
                path: "sample.bin",
                cursor: 0,
                cursor_label: None,
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
            200,
            &palette,
        );

        let warning_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("png edit may break crc"))
            .expect("message span");
        assert_eq!(warning_span.style.fg, Some(Color::Black));
        assert_eq!(warning_span.style.bg, Some(Color::Yellow));
        assert!(warning_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn notice_messages_use_notice_style() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                main_view_label: None,
                mode: Mode::Normal,
                path: "sample.bin",
                cursor: 0,
                cursor_label: None,
                display_len: 1,
                visible_len: 1,
                selection_span: None,
                selection_logical_len: None,
                paste_info: None,
                dirty: false,
                message: "wrapped search",
                message_level: StatusLevel::Notice,
                readonly: false,
            },
            200,
            &palette,
        );

        let notice_span = line.spans.last().expect("message span");
        assert_eq!(notice_span.style.fg, Some(Color::Cyan));
        assert!(notice_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn error_messages_use_error_style() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                main_view_label: None,
                mode: Mode::EditHex {
                    phase: NibblePhase::High,
                },
                path: "sample.bin",
                cursor: 0,
                cursor_label: None,
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
            200,
            &palette,
        );

        let error_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("document is read-only"))
            .expect("message span");
        assert_eq!(error_span.style.fg, Some(Color::White));
        assert_eq!(error_span.style.bg, Some(Color::Red));
        assert!(error_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn status_line_includes_visible_and_logical_selection_lengths() {
        let palette = Palette::new(ColorLevel::NoColor);
        let line = build(
            StatusInfo {
                main_view_label: Some("DIS"),
                mode: Mode::Normal,
                path: "sample.bin",
                cursor: 0x10,
                cursor_label: None,
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
            200,
            &palette,
        );

        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("len 12"));
        assert!(text.contains("vis 10"));
        assert!(text.contains("view DIS"));
        assert!(text.contains("sel(span) 4"));
        assert!(text.contains("sel(logical) 3"));
    }

    #[test]
    fn status_line_can_show_memory_virtual_address() {
        let palette = Palette::new(ColorLevel::NoColor);
        let line = build(
            StatusInfo {
                main_view_label: None,
                mode: Mode::Normal,
                path: "memory://4242/0x1000-0x1004",
                cursor: 2,
                cursor_label: Some("va 0x1002".to_owned()),
                display_len: 4,
                visible_len: 4,
                selection_span: None,
                selection_logical_len: None,
                paste_info: None,
                dirty: false,
                message: "",
                message_level: StatusLevel::Info,
                readonly: false,
            },
            200,
            &palette,
        );

        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("va 0x1002"));
        assert!(!text.contains("offset 0x2"));
    }

    #[test]
    fn warning_message_is_preserved_on_narrow_status_line() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                main_view_label: Some("DIS"),
                mode: Mode::Normal,
                path: "/very/long/path/that/should/not/hide/warnings/sample.bin",
                cursor: 0x14190,
                cursor_label: None,
                display_len: 845232,
                visible_len: 845232,
                selection_span: Some(128),
                selection_logical_len: Some(128),
                paste_info: Some("paste raw 4096 bytes"),
                dirty: true,
                message: "analysis outdated; rerun :ana before Sagitta symbol jumps",
                message_level: StatusLevel::Warning,
                readonly: false,
            },
            56,
            &palette,
        );

        let text = line_text(&line);
        assert!(text_width(&text) <= 56);
        assert!(text.contains("analysis outdated"));
        assert!(!text.contains("very/long/path"));
        let warning_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("analysis outdated"))
            .expect("warning message span");
        assert_eq!(warning_span.style.bg, Some(Color::Yellow));
    }

    #[test]
    fn long_info_message_is_preserved_on_narrow_status_line() {
        let palette = Palette::new(ColorLevel::NoColor);
        let line = build(
            StatusInfo {
                main_view_label: None,
                mode: Mode::Normal,
                path: "/very/long/path/that/should/not/hide/hash/results/sample.bin",
                cursor: 0x14190,
                cursor_label: None,
                display_len: 314572800,
                visible_len: 314572800,
                selection_span: None,
                selection_logical_len: None,
                paste_info: None,
                dirty: false,
                message: "sha256 [entire file]: 254bcc3fc4f27172636df4bf32de9f107f620d559b20d760197e452b97453917 (134217728 bytes) [copied]",
                message_level: StatusLevel::Info,
                readonly: false,
            },
            80,
            &palette,
        );

        let text = line_text(&line);
        assert!(text_width(&text) <= 80);
        assert!(text.contains("sha256 [entire file]"));
        assert!(text.contains('…'));
        assert!(!text.contains("very/long/path"));
    }

    #[test]
    fn long_notice_message_is_preserved_on_narrow_status_line() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                main_view_label: None,
                mode: Mode::Normal,
                path: "/very/long/path/that/should/not/hide/progress/sample.bin",
                cursor: 0,
                cursor_label: None,
                display_len: 314572800,
                visible_len: 314572800,
                selection_span: None,
                selection_logical_len: None,
                paste_info: None,
                dirty: false,
                message: "hashing sha256 [entire file]... 128 MiB / 300 MiB checked (43%); 128 MiB logical hashed; Esc to cancel",
                message_level: StatusLevel::Notice,
                readonly: false,
            },
            80,
            &palette,
        );

        let text = line_text(&line);
        assert!(text_width(&text) <= 80);
        assert!(text.contains("hashing sha256"));
        assert!(text.contains('…'));
        assert!(!text.contains("very/long/path"));
    }

    #[test]
    fn error_message_is_truncated_last_when_space_is_tight() {
        let palette = Palette::new(ColorLevel::Basic);
        let line = build(
            StatusInfo {
                main_view_label: Some("DIS"),
                mode: Mode::Normal,
                path: "/tmp/sample.bin",
                cursor: 0x14190,
                cursor_label: None,
                display_len: 845232,
                visible_len: 845232,
                selection_span: None,
                selection_logical_len: None,
                paste_info: None,
                dirty: false,
                message: "assembly error: invalid operand for selected architecture",
                message_level: StatusLevel::Error,
                readonly: false,
            },
            40,
            &palette,
        );

        let text = line_text(&line);
        assert!(text_width(&text) <= 40);
        assert!(text.contains("assembly error"));
        assert!(text.contains('…'));
        assert!(!text.contains("offset 0x14190"));
        let error_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("assembly error"))
            .expect("error message span");
        assert_eq!(error_span.style.bg, Some(Color::Red));
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}
