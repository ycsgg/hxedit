use ratatui::text::{Line, Span};

use crate::app::{BookmarkEntry, BookmarkState};
use crate::view::palette::Palette;

const MIN_DETAIL_ROWS: usize = 5;
const LIST_GAP_ROWS: usize = 1;

pub(crate) fn list_height(total_height: u16) -> usize {
    let total = total_height as usize;
    total.saturating_sub(MIN_DETAIL_ROWS + LIST_GAP_ROWS).max(1)
}

pub(crate) fn header_line(width: u16, palette: &Palette) -> Line<'static> {
    let width = width.max(1) as usize;
    let columns = ListColumns::for_width(width);
    Line::from(vec![
        Span::styled(pad_cell("Offset", columns.offset), palette.inspector_header),
        Span::raw("  "),
        Span::styled(pad_cell("Name", columns.name), palette.inspector_header),
    ])
}

pub(crate) fn build_visible_lines(
    state: &BookmarkState,
    selected_row: usize,
    start: usize,
    end: usize,
    width: u16,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let end = end.min(state.entries.len());
    if start >= end {
        return Vec::new();
    }
    state.entries[start..end]
        .iter()
        .enumerate()
        .map(|(row, entry)| list_line(entry, start + row == selected_row, width, palette))
        .collect()
}

pub(crate) fn detail_lines(
    state: &BookmarkState,
    width: u16,
    document_revision: u64,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let Some(entry) = state.selected_entry() else {
        return vec![Line::styled("No bookmarks", palette.inspector_value)];
    };
    let mut lines = Vec::new();
    lines.push(detail_line("id", &format!("#{}", entry.id), width, palette));
    lines.extend(wrap_detail("name", &entry.name, width, palette));
    lines.push(detail_line(
        "range",
        &format!("display 0x{:x}..0x{:x}", entry.start, entry.end()),
        width,
        palette,
    ));
    lines.push(detail_line("len", &entry.len.to_string(), width, palette));
    lines.push(detail_line("color", entry.color.label(), width, palette));
    if entry.created_revision != document_revision {
        lines.push(detail_line("state", "possibly stale", width, palette));
    }
    if let Some(note) = &entry.note {
        lines.extend(wrap_detail("note", note, width, palette));
    }
    lines
}

fn list_line(
    entry: &BookmarkEntry,
    selected: bool,
    width: usize,
    palette: &Palette,
) -> Line<'static> {
    let columns = ListColumns::for_width(width);
    let marker = if entry.note.is_some() { "●" } else { "◆" };
    let style = if selected {
        palette.inspector_active
    } else {
        palette.bookmark_marker(entry.color.palette_index())
    };
    let name = format!("{marker} {}", entry.name);
    Line::from(vec![
        Span::styled(
            pad_cell(&format!("0x{:08x}", entry.start), columns.offset),
            palette.gutter,
        ),
        Span::raw("  "),
        Span::styled(fit_cell(&name, columns.name), style),
    ])
}

fn detail_line(label: &'static str, value: &str, width: usize, palette: &Palette) -> Line<'static> {
    let label_width = 7;
    let value_width = width.saturating_sub(label_width + 1);
    Line::from(vec![
        Span::styled(format!("{label:<label_width$}"), palette.inspector_header),
        Span::raw(" "),
        Span::styled(
            truncate_with_ellipsis(value, value_width),
            palette.inspector_value,
        ),
    ])
}

fn wrap_detail(
    label: &'static str,
    value: &str,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let label_width = 7;
    let value_width = width.saturating_sub(label_width + 1).max(1);
    wrap_value(value, value_width)
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let label_text = if index == 0 { label } else { "" };
            Line::from(vec![
                Span::styled(
                    format!("{label_text:<label_width$}"),
                    palette.inspector_header,
                ),
                Span::raw(" "),
                Span::styled(chunk, palette.inspector_value),
            ])
        })
        .collect()
}

fn wrap_value(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    value
        .chars()
        .collect::<Vec<_>>()
        .chunks(width)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct ListColumns {
    offset: usize,
    name: usize,
}

impl ListColumns {
    fn for_width(width: usize) -> Self {
        let offset = 10;
        let gap = 2;
        Self {
            offset,
            name: width.saturating_sub(offset + gap).max(1),
        }
    }
}

fn fit_cell(value: &str, width: usize) -> String {
    let fitted = truncate_with_ellipsis(value, width);
    pad_cell(&fitted, width)
}

fn pad_cell(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count >= width {
        value.to_owned()
    } else {
        format!("{value:<width$}")
    }
}

fn truncate_with_ellipsis(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut out = value.chars().take(width - 1).collect::<String>();
    out.push('…');
    out
}
