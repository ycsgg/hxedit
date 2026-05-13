use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Block;

pub const MIN_SIDE_PANEL_WIDTH: u16 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainPaneKind {
    Hex,
    Disassembly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidePanelWidthPolicy {
    Normal,
    Half,
}

/// Top-level screen slices used by the app render pass.
#[derive(Debug, Clone, Copy)]
pub struct ScreenLayout {
    pub main: Rect,
    pub status: Rect,
    pub command: Option<Rect>,
}

/// Fixed column layout for the hex viewer body.
///
/// When show_side_panel is true, side_panel_sep and side_panel are Some.
#[derive(Debug, Clone, Copy)]
pub struct MainColumns {
    pub main_pane_kind: MainPaneKind,
    pub gutter: Rect,
    pub sep1: Rect,
    pub hex: Rect,
    pub sep2: Rect,
    pub ascii: Rect,
    /// Side-panel separator. Only present when the side panel is visible.
    pub side_panel_sep: Option<Rect>,
    /// Side-panel area. Hosts inspector / symbol / data pages.
    pub side_panel: Option<Rect>,
}

pub fn split_screen(area: Rect, show_command: bool) -> ScreenLayout {
    let constraints = if show_command {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(5),
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    ScreenLayout {
        main: sections[0],
        status: sections[1],
        // `then_some` is eager and would index sections[2] even when
        // `show_command` is false. Use the lazy variant to avoid panicking.
        command: show_command.then(|| sections[2]),
    }
}

pub fn split_main(
    block: &Block,
    area: Rect,
    gutter_width: u16,
    show_side_panel: bool,
    main_pane_kind: MainPaneKind,
    side_panel_policy: SidePanelWidthPolicy,
) -> MainColumns {
    let inner = block.inner(area);
    let (main_hex_fill, main_ascii_fill, inspector_fill) = match main_pane_kind {
        MainPaneKind::Hex => (3, 1, 2),
        MainPaneKind::Disassembly => (2, 4, 3),
    };

    if show_side_panel && inner.width >= MIN_SIDE_PANEL_WIDTH {
        if side_panel_policy == SidePanelWidthPolicy::Half {
            let sections = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Fill(1),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                ])
                .split(inner);
            let main_sections = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(gutter_width),
                    Constraint::Length(1),
                    Constraint::Fill(main_hex_fill),
                    Constraint::Length(1),
                    Constraint::Fill(main_ascii_fill),
                ])
                .split(sections[0]);
            return MainColumns {
                main_pane_kind,
                gutter: main_sections[0],
                sep1: main_sections[1],
                hex: main_sections[2],
                sep2: main_sections[3],
                ascii: main_sections[4],
                side_panel_sep: Some(sections[1]),
                side_panel: Some(sections[2]),
            };
        }

        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(gutter_width),
                Constraint::Length(1),
                Constraint::Fill(main_hex_fill),
                Constraint::Length(1),
                Constraint::Fill(main_ascii_fill),
                Constraint::Length(1),
                Constraint::Fill(inspector_fill),
            ])
            .split(inner);
        MainColumns {
            main_pane_kind,
            gutter: sections[0],
            sep1: sections[1],
            hex: sections[2],
            sep2: sections[3],
            ascii: sections[4],
            side_panel_sep: Some(sections[5]),
            side_panel: Some(sections[6]),
        }
    } else {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(gutter_width),
                Constraint::Length(1),
                Constraint::Fill(main_hex_fill),
                Constraint::Length(1),
                Constraint::Fill(main_ascii_fill),
            ])
            .split(inner);
        MainColumns {
            main_pane_kind,
            gutter: sections[0],
            sep1: sections[1],
            hex: sections[2],
            sep2: sections[3],
            ascii: sections[4],
            side_panel_sep: None,
            side_panel: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_screen_without_command_area_does_not_panic() {
        let layout = split_screen(Rect::new(0, 0, 80, 24), false);
        assert_eq!(layout.command, None);
    }

    #[test]
    fn split_screen_with_command_area_returns_bottom_slice() {
        let layout = split_screen(Rect::new(0, 0, 80, 24), true);
        assert!(layout.command.is_some());
    }

    #[test]
    fn disassembly_layout_gives_more_width_to_text_pane() {
        let area = Rect::new(0, 0, 120, 24);
        let block = Block::default();
        let columns = split_main(
            &block,
            area,
            8,
            false,
            MainPaneKind::Disassembly,
            SidePanelWidthPolicy::Normal,
        );
        assert!(columns.ascii.width > columns.hex.width);
    }

    #[test]
    fn hex_layout_keeps_hex_pane_at_least_as_wide_as_ascii() {
        let area = Rect::new(0, 0, 120, 24);
        let block = Block::default();
        let columns = split_main(
            &block,
            area,
            8,
            false,
            MainPaneKind::Hex,
            SidePanelWidthPolicy::Normal,
        );
        assert!(columns.hex.width >= columns.ascii.width);
    }

    #[test]
    fn hex_layout_with_inspector_matches_inspector_branch_proportions() {
        let area = Rect::new(0, 0, 120, 24);
        let block = Block::default();
        let columns = split_main(
            &block,
            area,
            8,
            true,
            MainPaneKind::Hex,
            SidePanelWidthPolicy::Normal,
        );
        let side_panel = columns.side_panel.expect("side panel visible");
        assert!(columns.hex.width > side_panel.width);
        assert!(side_panel.width > columns.ascii.width);
    }

    #[test]
    fn disassembly_layout_keeps_inspector_at_least_as_wide_as_hex_layout() {
        let area = Rect::new(0, 0, 120, 24);
        let block = Block::default();
        let hex = split_main(
            &block,
            area,
            8,
            true,
            MainPaneKind::Hex,
            SidePanelWidthPolicy::Normal,
        );
        let dis = split_main(
            &block,
            area,
            8,
            true,
            MainPaneKind::Disassembly,
            SidePanelWidthPolicy::Normal,
        );
        assert!(
            dis.side_panel.expect("dis side panel").width
                >= hex.side_panel.expect("hex side panel").width
        );
        assert!(dis.ascii.width > dis.hex.width);
    }

    #[test]
    fn half_side_panel_policy_splits_main_and_side_nearly_evenly() {
        let area = Rect::new(0, 0, 121, 24);
        let block = Block::default();
        let columns = split_main(
            &block,
            area,
            8,
            true,
            MainPaneKind::Hex,
            SidePanelWidthPolicy::Half,
        );
        let left_width = columns.gutter.width
            + columns.sep1.width
            + columns.hex.width
            + columns.sep2.width
            + columns.ascii.width;
        let side_width = columns.side_panel.expect("side panel").width;
        assert!(left_width.abs_diff(side_width) <= 1);
        assert_eq!(columns.side_panel_sep.expect("separator").width, 1);
    }
}
