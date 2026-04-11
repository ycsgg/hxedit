use ratatui::layout::Rect;

use crate::mode::NibblePhase;
use crate::view::layout::MainColumns;

/// Result of translating a terminal click into a byte selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseHit {
    pub offset: u64,
    pub phase: Option<NibblePhase>,
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
        return None;
    }

    let row = row_from_point(columns.gutter, x, y)
        .or_else(|| row_from_point(columns.hex, x, y))
        .or_else(|| row_from_point(columns.ascii, x, y))?;
    let row_offset = viewport_top + row as u64 * bytes_per_line as u64;

    let (col, phase) = if contains(columns.gutter, x, y) {
        (0, None)
    } else if contains(columns.hex, x, y) {
        hex_col_from_x(x - columns.hex.x, bytes_per_line)?
    } else if contains(columns.ascii, x, y) {
        (ascii_col_from_x(x - columns.ascii.x, bytes_per_line)?, None)
    } else {
        return None;
    };

    let offset = row_offset + col as u64;
    (offset < file_len).then_some(MouseHit { offset, phase })
}

fn row_from_point(rect: Rect, x: u16, y: u16) -> Option<usize> {
    contains(rect, x, y).then_some((y - rect.y) as usize)
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> MainColumns {
        MainColumns {
            gutter: Rect::new(0, 0, 8, 5),
            sep1: Rect::new(8, 0, 1, 5),
            hex: Rect::new(9, 0, 49, 5),
            sep2: Rect::new(58, 0, 1, 5),
            ascii: Rect::new(59, 0, 17, 5),
        }
    }

    #[test]
    fn gutter_click_selects_first_byte_of_row() {
        let hit = hit_test(columns(), 1, 2, 0x20, 16, 256).unwrap();
        assert_eq!(
            hit,
            MouseHit {
                offset: 0x20 + 32,
                phase: None,
            }
        );
    }

    #[test]
    fn hex_click_selects_byte_and_nibble() {
        let hit = hit_test(columns(), 10, 1, 0, 16, 256).unwrap();
        assert_eq!(
            hit,
            MouseHit {
                offset: 0x10,
                phase: Some(NibblePhase::Low),
            }
        );
    }

    #[test]
    fn ascii_click_selects_right_half_byte() {
        let hit = hit_test(columns(), 68, 0, 0, 16, 256).unwrap();
        assert_eq!(
            hit,
            MouseHit {
                offset: 8,
                phase: None,
            }
        );
    }
}
