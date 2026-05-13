use std::collections::HashMap;
use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, MainView, SidePanelKind};
use crate::commands::hints;
use crate::core::document::ByteSlot;
use crate::diff::{diff_sources, DiffByte, DiffHunkKind, DiffResult, DiffSource};
use crate::disasm::DisasmRow;
use crate::mode::Mode;
use crate::profile::{FrameStats, RenderMainStats};
use crate::util::format::offset_width;
use crate::view::{
    ascii_grid, command_line, data_panel, diff_panel, disasm_grid, gutter, hex_grid,
    inspector as inspector_view, layout, status, symbol_panel,
};

struct VisibleRows {
    offsets: Vec<u64>,
    rows: Vec<Vec<ByteSlot>>,
}

#[derive(Debug, Clone, Default)]
struct VisibleDiffPage {
    rows: Vec<diff_panel::DiffPanelRow>,
    overlay_spans: Vec<hex_grid::DiffOverlaySpan>,
    main_rows: Vec<Vec<hex_grid::HexGridCell>>,
    main_ascii_rows: Vec<Vec<(ByteSlot, Option<u64>, bool)>>,
    main_row_offsets: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffCellHit {
    pub side: crate::input::mouse::DiffCellSide,
    pub visual_offset: u64,
    pub current_display_offset: Option<u64>,
    pub other_offset: Option<u64>,
}

enum MainPaneLines {
    Hex {
        hex_header: Line<'static>,
        hex: Vec<Line<'static>>,
        ascii_header: Line<'static>,
        ascii: Vec<Line<'static>>,
    },
    Disassembly {
        bytes: Vec<Line<'static>>,
        text: Vec<Line<'static>>,
    },
}

struct MainLines {
    gutter: Vec<Line<'static>>,
    pane: MainPaneLines,
}

fn separator_widget(height: u16, palette: &crate::view::palette::Palette) -> Paragraph<'static> {
    let lines = (0..height)
        .map(|_| Line::styled("│", palette.separator))
        .collect::<Vec<_>>();
    Paragraph::new(lines)
}

fn top_header_area(area: Rect) -> Rect {
    Rect {
        height: area.height.min(1),
        ..area
    }
}

fn scrolled_body_area(area: Rect) -> Rect {
    Rect {
        y: area.y.saturating_add(1),
        height: area.height.saturating_sub(1),
        ..area
    }
}

mod diff_projection;
mod main_view;
mod side_panel;

#[cfg(test)]
mod tests;
