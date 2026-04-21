use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Block;

pub const MIN_INSPECTOR_WIDTH: u16 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainPaneKind {
    Hex,
    Disassembly,
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
/// When show_inspector is true, sep3 and inspector are Some.
#[derive(Debug, Clone, Copy)]
pub struct MainColumns {
    pub main_pane_kind: MainPaneKind,
    pub gutter: Rect,
    pub sep1: Rect,
    pub hex: Rect,
    pub sep2: Rect,
    pub ascii: Rect,
    /// Inspector separator. Only present when inspector is open.
    pub sep3: Option<Rect>,
    /// Inspector panel area. Only present when inspector is open.
    pub inspector: Option<Rect>,
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
    show_inspector: bool,
    main_pane_kind: MainPaneKind,
) -> MainColumns {
    let inner = block.inner(area);
    let (main_hex_fill, main_ascii_fill, inspector_fill) = match main_pane_kind {
        MainPaneKind::Hex => (3, 1, 2),
        MainPaneKind::Disassembly => (2, 4, 3),
    };

    if show_inspector && inner.width >= MIN_INSPECTOR_WIDTH {
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
            sep3: Some(sections[5]),
            inspector: Some(sections[6]),
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
            sep3: None,
            inspector: None,
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
        let columns = split_main(&block, area, 8, false, MainPaneKind::Disassembly);
        assert!(columns.ascii.width > columns.hex.width);
    }

    #[test]
    fn hex_layout_keeps_hex_pane_at_least_as_wide_as_ascii() {
        let area = Rect::new(0, 0, 120, 24);
        let block = Block::default();
        let columns = split_main(&block, area, 8, false, MainPaneKind::Hex);
        assert!(columns.hex.width >= columns.ascii.width);
    }

    #[test]
    fn hex_layout_with_inspector_matches_inspector_branch_proportions() {
        let area = Rect::new(0, 0, 120, 24);
        let block = Block::default();
        let columns = split_main(&block, area, 8, true, MainPaneKind::Hex);
        let inspector = columns.inspector.expect("inspector visible");
        assert!(columns.hex.width > inspector.width);
        assert!(inspector.width > columns.ascii.width);
    }

    #[test]
    fn disassembly_layout_keeps_inspector_at_least_as_wide_as_hex_layout() {
        let area = Rect::new(0, 0, 120, 24);
        let block = Block::default();
        let hex = split_main(&block, area, 8, true, MainPaneKind::Hex);
        let dis = split_main(&block, area, 8, true, MainPaneKind::Disassembly);
        assert!(
            dis.inspector.expect("dis inspector").width
                >= hex.inspector.expect("hex inspector").width
        );
        assert!(dis.ascii.width > dis.hex.width);
    }
}
