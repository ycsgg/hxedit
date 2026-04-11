use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Block;

/// Top-level screen slices used by the app render pass.
#[derive(Debug, Clone, Copy)]
pub struct ScreenLayout {
    pub main: Rect,
    pub status: Rect,
    pub command: Option<Rect>,
}

/// Fixed three-column layout for the hex viewer body.
#[derive(Debug, Clone, Copy)]
pub struct MainColumns {
    pub gutter: Rect,
    pub sep1: Rect,
    pub hex: Rect,
    pub sep2: Rect,
    pub ascii: Rect,
}

pub fn split_screen(area: Rect, show_command: bool) -> ScreenLayout {
    let constraints = if show_command {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
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

pub fn split_main(block: &Block, area: Rect, gutter_width: u16) -> MainColumns {
    let inner = block.inner(area);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(gutter_width),
            Constraint::Length(1),
            Constraint::Fill(3),
            Constraint::Length(1),
            Constraint::Fill(2),
        ])
        .split(inner);
    MainColumns {
        gutter: sections[0],
        sep1: sections[1],
        hex: sections[2],
        sep2: sections[3],
        ascii: sections[4],
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
}
