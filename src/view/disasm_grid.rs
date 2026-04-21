use ratatui::text::{Line, Span};

use crate::core::document::ByteSlot;
use crate::view::palette::Palette;

pub fn build_bytes(
    rows: &[Vec<ByteSlot>],
    row_offsets: &[u64],
    palette: &Palette,
) -> Vec<Line<'static>> {
    rows.iter()
        .enumerate()
        .map(|(row_idx, row)| {
            let offset = row_offsets.get(row_idx).copied().unwrap_or_default();
            let mut spans = Vec::new();
            let mut rendered = 0usize;
            for slot in row {
                match slot {
                    ByteSlot::Present(byte) => {
                        if rendered > 0 {
                            spans.push(Span::raw(" "));
                        }
                        spans.push(Span::styled(format!("{byte:02x}"), palette.status));
                        rendered += 1;
                        if rendered >= 8 {
                            break;
                        }
                    }
                    ByteSlot::Deleted => {
                        if rendered > 0 {
                            spans.push(Span::raw(" "));
                        }
                        spans.push(Span::styled("XX", palette.warning));
                        rendered += 1;
                        if rendered >= 8 {
                            break;
                        }
                    }
                    ByteSlot::Empty => break,
                }
            }
            if rendered == 0 {
                spans.push(Span::styled("--", palette.separator));
            }
            let count = row
                .iter()
                .take_while(|slot| !matches!(slot, ByteSlot::Empty))
                .count();
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("; {} bytes @ 0x{offset:x}", count),
                palette.separator,
            ));
            Line::from(spans)
        })
        .collect()
}

pub fn build_text(lines: &[String], palette: &Palette) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| Line::from(vec![Span::styled(line.clone(), palette.status)]))
        .collect()
}
