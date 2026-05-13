use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::core::document::ByteSlot;
use crate::mode::{Mode, NibblePhase};
use crate::util::format::hex_pair;
use crate::view::byte_style::slot_style;
use crate::view::palette::Palette;

#[derive(Debug, Clone, Default)]
pub struct HexGridOverlays {
    pub diff_spans: Vec<DiffOverlaySpan>,
    pub selection: Option<(u64, u64)>,
    pub inspector_highlight: Option<(u64, u64)>,
    pub search_matches: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOverlaySpan {
    pub start: u64,
    pub end: u64,
    pub kind: DiffOverlayKind,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOverlayKind {
    Replace,
    OnlyCurrent,
    OnlyOther,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexGridCell {
    pub slot: ByteSlot,
    pub display_offset: Option<u64>,
    pub diff: Option<DiffOverlayKind>,
    /// Display slot whose visual column this projected diff cell occupies.
    ///
    /// For normal current-side bytes this matches `display_offset`. For
    /// `OnlyOther` placeholders (`__`) there is no current byte, but mouse
    /// hit-testing and row layout still need a stable visual slot so the
    /// placeholder is counted in column calculations.
    pub visual_offset: Option<u64>,
    pub other_offset: Option<u64>,
}

pub fn build(
    rows: &[Vec<ByteSlot>],
    row_offsets: &[u64],
    cursor: u64,
    mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
    overlays: HexGridOverlays,
) -> Vec<Line<'static>> {
    let projected = rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| {
            row.iter()
                .enumerate()
                .map(|(col_idx, slot)| {
                    let offset = row_offsets[row_idx] + col_idx as u64;
                    HexGridCell {
                        slot: *slot,
                        display_offset: Some(offset),
                        diff: None,
                        visual_offset: Some(offset),
                        other_offset: None,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    build_projected(&projected, cursor, mode, palette, bytes_per_line, overlays)
}

pub fn build_projected(
    rows: &[Vec<HexGridCell>],
    cursor: u64,
    mode: Mode,
    palette: &Palette,
    bytes_per_line: usize,
    overlays: HexGridOverlays,
) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| {
            let mut spans = Vec::with_capacity(bytes_per_line * 4);
            for (col_idx, cell) in row.iter().enumerate() {
                let mut base = slot_style(cell.slot, palette);
                let diff = cell.diff.or_else(|| {
                    cell.display_offset
                        .and_then(|offset| diff_overlay_at(&overlays.diff_spans, offset))
                        .map(|diff| diff.kind)
                });
                if let Some(diff) = diff {
                    base = base.patch(diff_style(diff, palette));
                }
                let diff_active = cell
                    .display_offset
                    .and_then(|offset| diff_overlay_at(&overlays.diff_spans, offset))
                    .is_some_and(|diff| diff.active)
                    || (cell.diff.is_some()
                        && cell.display_offset.is_none()
                        && cell.visual_offset == Some(cursor));
                if diff_active {
                    base = palette.diff_active.patch(base);
                }
                if let Some(offset) = cell.display_offset {
                    if highlighted(overlays.inspector_highlight, offset) {
                        base = palette.inspector_highlight.patch(base);
                    }
                    if highlighted_any(&overlays.search_matches, offset) {
                        base = palette.search_hit.patch(base);
                    }
                    if selected(overlays.selection, offset) {
                        base = palette.selection.patch(base);
                    }
                }
                let pair = if diff == Some(DiffOverlayKind::OnlyOther) {
                    ['_', '_']
                } else {
                    hex_pair(cell.slot)
                };
                let is_cursor = cell
                    .display_offset
                    .map(|offset| offset == cursor)
                    .unwrap_or(false);
                let phase = match mode {
                    Mode::EditHex { phase } if is_cursor => Some(phase),
                    _ => None,
                };

                spans.push(Span::styled(
                    pair[0].to_string(),
                    style_for_nibble(base, is_cursor, phase, true, palette),
                ));
                spans.push(Span::styled(
                    pair[1].to_string(),
                    style_for_nibble(base, is_cursor, phase, false, palette),
                ));

                if col_idx + 1 != row.len() {
                    if bytes_per_line >= 8 && col_idx + 1 == bytes_per_line / 2 {
                        spans.push(Span::styled(" │ ", palette.separator));
                    } else {
                        spans.push(Span::raw(" "));
                    }
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn selected(selection: Option<(u64, u64)>, offset: u64) -> bool {
    selection
        .map(|(start, end)| offset >= start && offset <= end)
        .unwrap_or(false)
}

fn highlighted(highlight: Option<(u64, u64)>, offset: u64) -> bool {
    highlight
        .map(|(start, end)| offset >= start && offset <= end)
        .unwrap_or(false)
}

fn highlighted_any(highlights: &[(u64, u64)], offset: u64) -> bool {
    highlights
        .iter()
        .any(|(start, end)| offset >= *start && offset <= *end)
}

fn diff_overlay_at(overlays: &[DiffOverlaySpan], offset: u64) -> Option<DiffOverlaySpan> {
    overlays
        .iter()
        .copied()
        .find(|overlay| offset >= overlay.start && offset <= overlay.end)
}

fn diff_style(kind: DiffOverlayKind, palette: &Palette) -> Style {
    match kind {
        DiffOverlayKind::Replace => palette.diff_replace,
        DiffOverlayKind::OnlyCurrent => palette.diff_only_current,
        DiffOverlayKind::OnlyOther => palette.diff_only_other,
        DiffOverlayKind::Unresolved => palette.diff_unresolved,
    }
}

fn style_for_nibble(
    base: Style,
    is_cursor: bool,
    phase: Option<NibblePhase>,
    is_high: bool,
    palette: &Palette,
) -> Style {
    if !is_cursor {
        return base;
    }
    match phase {
        Some(NibblePhase::High) if is_high => base.patch(palette.cursor_nibble),
        Some(NibblePhase::Low) if !is_high => base.patch(palette.cursor_nibble),
        Some(_) => base.patch(palette.cursor),
        None => base.patch(palette.cursor),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::{build, DiffOverlayKind, DiffOverlaySpan, HexGridOverlays};
    use crate::core::document::ByteSlot;
    use crate::mode::Mode;
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn inspector_highlight_underlines_selected_field_bytes() {
        let lines = build(
            &[vec![ByteSlot::Present(0x41), ByteSlot::Present(0x42)]],
            &[0],
            99,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            2,
            HexGridOverlays {
                diff_spans: Vec::new(),
                selection: None,
                inspector_highlight: Some((1, 1)),
                search_matches: Vec::new(),
            },
        );

        let line = &lines[0];
        assert!(!line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[3]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn cursor_keeps_field_highlight_modifier() {
        let lines = build(
            &[vec![ByteSlot::Present(0x41)]],
            &[0],
            0,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            1,
            HexGridOverlays {
                diff_spans: Vec::new(),
                selection: None,
                inspector_highlight: Some((0, 0)),
                search_matches: Vec::new(),
            },
        );

        let line = &lines[0];
        assert!(line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[1]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn search_matches_underlines_all_hit_bytes() {
        let lines = build(
            &[vec![
                ByteSlot::Present(0x41),
                ByteSlot::Present(0x42),
                ByteSlot::Present(0x43),
            ]],
            &[0],
            99,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            3,
            HexGridOverlays {
                diff_spans: Vec::new(),
                selection: None,
                inspector_highlight: None,
                search_matches: vec![(1, 2)],
            },
        );

        let line = &lines[0];
        assert!(!line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[3]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[4]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[6]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn cursor_keeps_search_highlight_modifier() {
        let lines = build(
            &[vec![ByteSlot::Present(0x41)]],
            &[0],
            0,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            1,
            HexGridOverlays {
                diff_spans: Vec::new(),
                selection: None,
                inspector_highlight: None,
                search_matches: vec![(0, 0)],
            },
        );

        let line = &lines[0];
        assert!(line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(line.spans[1]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn diff_overlay_applies_before_cursor_priority() {
        let lines = build(
            &[vec![ByteSlot::Present(0x41)]],
            &[0],
            0,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            1,
            HexGridOverlays {
                diff_spans: vec![DiffOverlaySpan {
                    start: 0,
                    end: 0,
                    kind: DiffOverlayKind::Replace,
                    active: false,
                }],
                selection: None,
                inspector_highlight: None,
                search_matches: Vec::new(),
            },
        );

        let line = &lines[0];
        assert_eq!(line.spans[0].style.bg, Some(Color::Blue));
        assert_eq!(line.spans[1].style.bg, Some(Color::Blue));
    }

    #[test]
    fn diff_overlay_marks_non_cursor_bytes() {
        let lines = build(
            &[vec![ByteSlot::Present(0x41), ByteSlot::Present(0x42)]],
            &[0],
            99,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            2,
            HexGridOverlays {
                diff_spans: vec![DiffOverlaySpan {
                    start: 1,
                    end: 1,
                    kind: DiffOverlayKind::OnlyCurrent,
                    active: false,
                }],
                selection: None,
                inspector_highlight: None,
                search_matches: Vec::new(),
            },
        );

        let line = &lines[0];
        assert_ne!(line.spans[0].style.bg, Some(Color::Red));
        assert_eq!(line.spans[3].style.bg, Some(Color::Red));
        assert_eq!(line.spans[4].style.bg, Some(Color::Red));
    }

    #[test]
    fn only_other_overlay_renders_left_placeholder() {
        let lines = build(
            &[vec![ByteSlot::Empty]],
            &[4],
            99,
            Mode::Normal,
            &Palette::new(ColorLevel::Basic),
            1,
            HexGridOverlays {
                diff_spans: vec![DiffOverlaySpan {
                    start: 4,
                    end: 4,
                    kind: DiffOverlayKind::OnlyOther,
                    active: false,
                }],
                selection: None,
                inspector_highlight: None,
                search_matches: Vec::new(),
            },
        );

        let line = &lines[0];
        assert_eq!(line.spans[0].content.as_ref(), "_");
        assert_eq!(line.spans[1].content.as_ref(), "_");
        assert_eq!(line.spans[0].style.bg, Some(Color::Red));
        assert_eq!(line.spans[1].style.bg, Some(Color::Red));
    }
}
