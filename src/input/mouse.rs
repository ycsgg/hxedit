use ratatui::layout::Rect;

use crate::disasm::DisasmRow;
use crate::mode::NibblePhase;
use crate::util::geometry::rect_contains;
use crate::view::layout::MainColumns;

/// Result of translating a terminal click into a byte selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseHit {
    pub offset: u64,
    pub phase: Option<NibblePhase>,
    pub side_panel_row: Option<usize>,
    pub diff_side: Option<DiffCellSide>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffCellSide {
    Current,
    Other,
}

pub fn disassembly_hit_test(
    columns: MainColumns,
    x: u16,
    y: u16,
    rows: &[DisasmRow],
) -> Option<MouseHit> {
    if let Some(side_panel) = columns.side_panel {
        if rect_contains(side_panel, x, y) {
            return side_panel_hit(side_panel, y);
        }
    }

    let row_idx = row_from_point(columns.gutter, x, y)
        .or_else(|| row_from_point(columns.hex, x, y))
        .or_else(|| row_from_point(columns.ascii, x, y))?;
    let row = rows.get(row_idx)?;

    let (offset, phase) =
        if rect_contains(columns.gutter, x, y) || rect_contains(columns.ascii, x, y) {
            (row.offset, None)
        } else if rect_contains(columns.hex, x, y) {
            let (byte_idx, phase) = disasm_hex_col_from_x(x - columns.hex.x, row.bytes.len())?;
            (row.offset + byte_idx as u64, phase)
        } else {
            return None;
        };

    Some(MouseHit {
        offset,
        phase,
        side_panel_row: None,
        diff_side: None,
    })
}

pub fn hit_test(
    columns: MainColumns,
    x: u16,
    y: u16,
    viewport_top: u64,
    bytes_per_line: usize,
    file_len: u64,
) -> Option<MouseHit> {
    if file_len == 0 || bytes_per_line == 0 {
        if let Some(side_panel) = columns.side_panel {
            if rect_contains(side_panel, x, y) {
                return side_panel_hit(side_panel, y);
            }
        }
        return None;
    }

    if let Some(side_panel) = columns.side_panel {
        if rect_contains(side_panel, x, y) {
            return side_panel_hit(side_panel, y);
        }
    }

    let body = hex_body_columns(columns);
    let row = row_from_point(body.gutter, x, y)
        .or_else(|| row_from_point(body.hex, x, y))
        .or_else(|| row_from_point(body.ascii, x, y))?;
    let row_offset = viewport_top + row as u64 * bytes_per_line as u64;

    let (col, phase) = if rect_contains(body.gutter, x, y) {
        (0, None)
    } else if rect_contains(body.hex, x, y) {
        hex_col_from_x(x - body.hex.x, bytes_per_line)?
    } else if rect_contains(body.ascii, x, y) {
        (ascii_col_from_x(x - body.ascii.x, bytes_per_line)?, None)
    } else {
        return None;
    };

    let offset = row_offset + col as u64;
    (offset < file_len).then_some(MouseHit {
        offset,
        phase,
        side_panel_row: None,
        diff_side: None,
    })
}

pub fn hit_test_diff_projected(
    columns: MainColumns,
    x: u16,
    y: u16,
    viewport_top: u64,
    bytes_per_line: usize,
    visual_len: u64,
) -> Option<MouseHit> {
    if visual_len == 0 || bytes_per_line == 0 {
        if let Some(side_panel) = columns.side_panel {
            if rect_contains(side_panel, x, y) {
                return side_panel_hit(side_panel, y);
            }
        }
        return None;
    }

    if let Some(side_panel) = columns.side_panel {
        if rect_contains(side_panel, x, y) {
            return side_panel_hit(side_panel, y);
        }
    }

    let body = hex_body_columns(columns);
    let (row, col, phase, diff_side) = if let Some(row) = row_from_point(body.gutter, x, y) {
        (row, 0, None, Some(DiffCellSide::Current))
    } else if let Some(row) = row_from_point(body.hex, x, y) {
        let (col, phase) = hex_col_from_x(x - body.hex.x, bytes_per_line)?;
        (row, col, phase, Some(DiffCellSide::Current))
    } else if let Some(row) = row_from_point(body.ascii, x, y) {
        (
            row,
            ascii_col_from_x(x - body.ascii.x, bytes_per_line)?,
            None,
            Some(DiffCellSide::Current),
        )
    } else {
        return None;
    };

    let offset = viewport_top
        .saturating_add(row as u64 * bytes_per_line as u64)
        .saturating_add(col as u64);
    (offset < visual_len).then_some(MouseHit {
        offset,
        phase,
        side_panel_row: None,
        diff_side,
    })
}

fn side_panel_hit(side_panel: Rect, y: u16) -> Option<MouseHit> {
    let line = y.saturating_sub(side_panel.y) as usize;
    if line == 0 {
        return None;
    }
    Some(MouseHit {
        offset: 0,
        phase: None,
        side_panel_row: Some(line - 1),
        diff_side: Some(DiffCellSide::Other),
    })
}

fn hex_body_columns(mut columns: MainColumns) -> MainColumns {
    columns.gutter = hex_body_rect(columns.gutter);
    columns.sep1 = hex_body_rect(columns.sep1);
    columns.hex = hex_body_rect(columns.hex);
    columns.sep2 = hex_body_rect(columns.sep2);
    columns.ascii = hex_body_rect(columns.ascii);
    columns
}

fn hex_body_rect(rect: Rect) -> Rect {
    Rect {
        y: rect.y.saturating_add(1),
        height: rect.height.saturating_sub(1),
        ..rect
    }
}

fn row_from_point(rect: Rect, x: u16, y: u16) -> Option<usize> {
    rect_contains(rect, x, y).then(|| (y - rect.y) as usize)
}

fn ascii_col_from_x(x: u16, bytes_per_line: usize) -> Option<usize> {
    let half = bytes_per_line / 2;
    if bytes_per_line >= 8 {
        if x < half as u16 {
            Some(x as usize)
        } else if x == half as u16 {
            Some(half)
        } else {
            let col = x as usize - 1;
            (col < bytes_per_line).then_some(col)
        }
    } else {
        let col = x as usize;
        (col < bytes_per_line).then_some(col)
    }
}

fn hex_col_from_x(x: u16, bytes_per_line: usize) -> Option<(usize, Option<NibblePhase>)> {
    let mut cursor_x = 0_u16;
    for col in 0..bytes_per_line {
        let separator_width = if col + 1 == bytes_per_line {
            0
        } else if bytes_per_line >= 8 && col + 1 == bytes_per_line / 2 {
            3
        } else {
            1
        };
        let cell_width = 2 + separator_width;
        if x >= cursor_x && x < cursor_x + cell_width {
            let rel = x - cursor_x;
            let phase = match rel {
                0 => Some(NibblePhase::High),
                1 => Some(NibblePhase::Low),
                _ => None,
            };
            return Some((col, phase));
        }
        cursor_x += cell_width;
    }
    None
}

fn disasm_hex_col_from_x(x: u16, byte_count: usize) -> Option<(usize, Option<NibblePhase>)> {
    let mut cursor_x = 0_u16;
    for col in 0..byte_count {
        let separator_width = usize::from(col + 1 < byte_count) as u16;
        let cell_width = 2 + separator_width;
        if x >= cursor_x && x < cursor_x + cell_width {
            let rel = x - cursor_x;
            let phase = match rel {
                0 => Some(NibblePhase::High),
                1 => Some(NibblePhase::Low),
                _ => None,
            };
            return Some((col, phase));
        }
        cursor_x += cell_width;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disasm::{DisasmRow, DisasmRowKind};

    fn columns() -> MainColumns {
        MainColumns {
            main_pane_kind: crate::view::layout::MainPaneKind::Hex,
            gutter: Rect::new(0, 0, 8, 5),
            sep1: Rect::new(8, 0, 1, 5),
            hex: Rect::new(9, 0, 49, 5),
            sep2: Rect::new(58, 0, 1, 5),
            ascii: Rect::new(59, 0, 17, 5),
            side_panel_sep: None,
            side_panel: None,
        }
    }

    fn disassembly_columns() -> MainColumns {
        MainColumns {
            main_pane_kind: crate::view::layout::MainPaneKind::Disassembly,
            gutter: Rect::new(0, 0, 18, 5),
            sep1: Rect::new(18, 0, 1, 5),
            hex: Rect::new(19, 0, 24, 5),
            sep2: Rect::new(43, 0, 1, 5),
            ascii: Rect::new(44, 0, 30, 5),
            side_panel_sep: None,
            side_panel: None,
        }
    }

    fn disassembly_rows() -> Vec<DisasmRow> {
        vec![
            DisasmRow {
                offset: 0x100,
                virtual_address: Some(0x401000),
                bytes: vec![0x55],
                text: "push rbp".to_owned(),
                assembly_text: "push rbp".to_owned(),
                symbolized_names: Vec::new(),
                symbol_label: None,
                direct_target: None,
                span_name: Some(".text".to_owned()),
                kind: DisasmRowKind::Instruction,
            },
            DisasmRow {
                offset: 0x101,
                virtual_address: Some(0x401001),
                bytes: vec![0x48, 0x89, 0xe5],
                text: "mov rbp, rsp".to_owned(),
                assembly_text: "mov rbp, rsp".to_owned(),
                symbolized_names: Vec::new(),
                symbol_label: None,
                direct_target: None,
                span_name: Some(".text".to_owned()),
                kind: DisasmRowKind::Instruction,
            },
            DisasmRow {
                offset: 0x200,
                virtual_address: None,
                bytes: vec![0x41, 0x42, 0x43],
                text: ".db 0x41, 0x42, 0x43".to_owned(),
                assembly_text: ".db 0x41, 0x42, 0x43".to_owned(),
                symbolized_names: Vec::new(),
                symbol_label: None,
                direct_target: None,
                span_name: Some(".rodata".to_owned()),
                kind: DisasmRowKind::Data,
            },
        ]
    }

    #[test]
    fn gutter_click_selects_first_byte_of_row() {
        let hit = hit_test(columns(), 1, 3, 0x20, 16, 256).unwrap();
        assert_eq!(
            hit,
            MouseHit {
                offset: 0x20 + 32,
                phase: None,
                side_panel_row: None,
                diff_side: None,
            }
        );
    }

    #[test]
    fn hex_click_selects_byte_and_nibble() {
        let hit = hit_test(columns(), 10, 2, 0, 16, 256).unwrap();
        assert_eq!(
            hit,
            MouseHit {
                offset: 0x10,
                phase: Some(NibblePhase::Low),
                side_panel_row: None,
                diff_side: None,
            }
        );
    }

    #[test]
    fn ascii_click_selects_right_half_byte() {
        let hit = hit_test(columns(), 68, 1, 0, 16, 256).unwrap();
        assert_eq!(
            hit,
            MouseHit {
                offset: 8,
                phase: None,
                side_panel_row: None,
                diff_side: None,
            }
        );
    }

    #[test]
    fn inspector_click_returns_visible_row() {
        let mut columns = columns();
        columns.side_panel = Some(Rect::new(76, 0, 16, 5));
        let hit = hit_test(columns, 80, 3, 0, 16, 256).unwrap();
        assert_eq!(
            hit,
            MouseHit {
                offset: 0,
                phase: None,
                side_panel_row: Some(2),
                diff_side: Some(DiffCellSide::Other),
            }
        );
    }

    #[test]
    fn inspector_title_row_does_not_select_a_field() {
        let mut columns = columns();
        columns.side_panel = Some(Rect::new(76, 0, 16, 5));
        assert_eq!(hit_test(columns, 80, 0, 0, 16, 256), None);
    }

    #[test]
    fn diff_projected_hit_test_counts_projected_cells() {
        let hit = hit_test_diff_projected(columns(), 18, 1, 0xb0, 16, 0xc0).unwrap();
        assert_eq!(hit.offset, 0xb3);
        assert_eq!(hit.phase, Some(NibblePhase::High));
        assert_eq!(hit.diff_side, Some(DiffCellSide::Current));
    }

    #[test]
    fn hex_column_header_is_not_a_clickable_data_row() {
        assert_eq!(hit_test(columns(), 10, 0, 0, 16, 256), None);
    }

    #[test]
    fn hit_test_outside_rect_does_not_panic() {
        assert_eq!(hit_test(columns(), 0, 10, 0, 16, 256), None);
    }

    #[test]
    fn disassembly_gutter_click_selects_row_start() {
        let hit = disassembly_hit_test(disassembly_columns(), 2, 1, &disassembly_rows()).unwrap();
        assert_eq!(hit.offset, 0x101);
        assert_eq!(hit.phase, None);
    }

    #[test]
    fn disassembly_hex_click_selects_byte_and_nibble() {
        let hit = disassembly_hit_test(disassembly_columns(), 22, 1, &disassembly_rows()).unwrap();
        assert_eq!(hit.offset, 0x102);
        assert_eq!(hit.phase, Some(NibblePhase::High));
    }

    #[test]
    fn disassembly_text_click_selects_row_start() {
        let hit = disassembly_hit_test(disassembly_columns(), 50, 2, &disassembly_rows()).unwrap();
        assert_eq!(hit.offset, 0x200);
        assert_eq!(hit.phase, None);
    }
}
