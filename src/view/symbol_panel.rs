use ratatui::text::{Line, Span};

use crate::app::symbol_state::SymbolPanelEntry;
use crate::app::SymbolState;
use crate::executable::{SymbolSource, SymbolType};
use crate::view::palette::Palette;

const MIN_DETAIL_ROWS: usize = 4;
const LIST_GAP_ROWS: usize = 1;

#[derive(Debug, Clone)]
pub(crate) struct SymbolLine {
    pub line: Line<'static>,
}

pub(crate) fn build_lines(
    state: &SymbolState,
    selected_row: usize,
    width: u16,
    palette: &Palette,
) -> Vec<SymbolLine> {
    let width = width.max(1) as usize;
    state
        .entries()
        .iter()
        .enumerate()
        .map(|(row_index, entry)| {
            let selected = row_index == selected_row;
            SymbolLine {
                line: list_line(entry, selected, width, palette),
            }
        })
        .collect()
}

pub(crate) fn list_height(total_height: u16) -> usize {
    let total = total_height as usize;
    total.saturating_sub(MIN_DETAIL_ROWS + LIST_GAP_ROWS).max(1)
}

pub(crate) fn detail_height(total_height: u16) -> usize {
    let total = total_height as usize;
    total
        .saturating_sub(list_height(total_height) + LIST_GAP_ROWS)
        .max(1)
}

pub(crate) fn header_line(width: u16, palette: &Palette) -> Line<'static> {
    let width = width.max(1) as usize;
    let columns = ListColumns::for_width(width);
    Line::from(vec![
        Span::styled(pad_cell("Address", columns.addr), palette.inspector_header),
        Span::raw("  "),
        Span::styled(pad_cell("Name", columns.name), palette.inspector_header),
    ])
}

pub(crate) fn detail_lines(
    state: &SymbolState,
    width: u16,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let entries = state.entries();
    let Some(entry) = entries.get(state.selected_row) else {
        return vec![Line::styled("No symbols", palette.inspector_value)];
    };

    let mut lines = Vec::with_capacity(MIN_DETAIL_ROWS);
    lines.extend(wrap_detail("symbol", &entry.name, width, palette));
    lines.push(detail_line(
        "meta",
        &format!(
            "{}/{}  {}  size {}  {}",
            state.selected_row + 1,
            entries.len(),
            symbol_type_label(entry.symbol_type),
            size_label(entry.size),
            source_label(entry.source)
        ),
        width,
        palette,
    ));
    lines.push(detail_line(
        "offset",
        &format!("0x{:x}", entry.address),
        width,
        palette,
    ));
    lines.push(detail_line(
        "file",
        &entry
            .file_offset
            .map(|offset| format!("0x{offset:x}"))
            .unwrap_or_else(|| "unmapped".to_owned()),
        width,
        palette,
    ));
    lines
}

pub(crate) fn detail_line_count(state: &SymbolState, width: u16) -> usize {
    let width = width.max(1) as usize;
    let entries = state.entries();
    let Some(entry) = entries.get(state.selected_row) else {
        return 1;
    };
    3 + wrap_value(&entry.name, detail_value_width(width)).len()
}

fn list_line(
    entry: &SymbolPanelEntry,
    selected: bool,
    width: usize,
    palette: &Palette,
) -> Line<'static> {
    let columns = ListColumns::for_width(width);
    let addr = format!("0x{:08x}", entry.address);
    let name_style = if selected {
        palette.inspector_active
    } else {
        palette.inspector_field
    };
    Line::from(vec![
        Span::styled(pad_cell(&addr, columns.addr), palette.gutter),
        Span::raw("  "),
        Span::styled(fit_cell(&entry.name, columns.name), name_style),
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
    let value_width = detail_value_width(width);
    let chunks = wrap_value(value, value_width);
    chunks
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

fn detail_value_width(width: usize) -> usize {
    let label_width = 7;
    width.saturating_sub(label_width + 1).max(1)
}

fn wrap_value(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    let chars: Vec<char> = value.chars().collect();
    chars
        .chunks(width)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn symbol_type_label(t: SymbolType) -> &'static str {
    match t {
        SymbolType::Function => "FUNC",
        SymbolType::Object => "DATA",
        SymbolType::Section => "SECT",
        SymbolType::Unknown => "-",
    }
}

fn source_label(s: SymbolSource) -> &'static str {
    match s {
        SymbolSource::Object => "static",
        SymbolSource::Dynamic => "dyn",
        SymbolSource::Export => "export",
    }
}

fn size_label(size: u64) -> String {
    if size > 0 {
        size.to_string()
    } else {
        "-".to_owned()
    }
}

#[derive(Debug, Clone, Copy)]
struct ListColumns {
    addr: usize,
    name: usize,
}

impl ListColumns {
    fn for_width(width: usize) -> Self {
        let addr = 10;
        let gap = 2;
        Self {
            addr,
            name: width.saturating_sub(addr + gap).max(1),
        }
    }
}

fn fit_cell(value: &str, width: usize) -> String {
    let fitted = truncate_with_ellipsis(value, width);
    pad_cell(&fitted, width)
}

fn pad_cell(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn truncate_with_ellipsis(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut output: String = value.chars().take(width - 1).collect();
    output.push('…');
    output
}
