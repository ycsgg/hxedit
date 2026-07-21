use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use tempfile::tempdir;

use crate::action::Action;
use crate::app::SidePanelKind;
use crate::commands::types::{BookmarkColorArg, BookmarkCommand, Command};
use crate::mode::Mode;

use super::{app_with_bytes, app_with_inspector_field};

#[test]
fn bookmark_add_uses_cursor_and_opens_panel() {
    let mut app = app_with_bytes(b"abcdef");
    app.cursor = 2;

    app.execute_command(Command::Bookmark(BookmarkCommand::Add {
        name: Some("mid".to_owned()),
        start: None,
        len: None,
        color: BookmarkColorArg::Default,
        note: Some("cursor note".to_owned()),
    }))
    .unwrap();

    assert_eq!(app.bookmark_state().entries.len(), 1);
    let entry = &app.bookmark_state().entries[0];
    assert_eq!(entry.name, "mid");
    assert_eq!(entry.start, 2);
    assert_eq!(entry.len, 1);
    assert_eq!(entry.note.as_deref(), Some("cursor note"));
    assert_eq!(app.active_side_panel, SidePanelKind::Bookmarks);
    assert_eq!(app.mode, Mode::SidePanel);
}

#[test]
fn bookmark_add_uses_visual_selection_range() {
    let mut app = app_with_bytes(b"abcdef");
    app.cursor = 4;
    app.selection_anchor = Some(1);
    app.mode = Mode::Visual;

    app.execute_command(Command::Bookmark(BookmarkCommand::Add {
        name: Some("sel".to_owned()),
        start: None,
        len: None,
        color: BookmarkColorArg::Yellow,
        note: None,
    }))
    .unwrap();

    let entry = &app.bookmark_state().entries[0];
    assert_eq!(entry.start, 1);
    assert_eq!(entry.len, 4);
    assert_eq!(app.selection_range(), None);
    assert_eq!(app.mode, Mode::SidePanel);
}

#[test]
fn bookmark_add_uses_inspector_field_range() {
    let mut app = app_with_inspector_field(b"abcdef", 2, 3);

    app.execute_command(Command::Bookmark(BookmarkCommand::Add {
        name: Some("field".to_owned()),
        start: None,
        len: None,
        color: BookmarkColorArg::Green,
        note: None,
    }))
    .unwrap();

    let entry = &app.bookmark_state().entries[0];
    assert_eq!(entry.start, 2);
    assert_eq!(entry.len, 3);
}

#[test]
fn bookmark_note_goto_next_prev_and_delete() {
    let mut app = app_with_bytes(b"0123456789");

    for (name, start) in [("a", 1), ("b", 5), ("c", 8)] {
        app.execute_command(Command::Bookmark(BookmarkCommand::Add {
            name: Some(name.to_owned()),
            start: Some(start),
            len: Some(1),
            color: BookmarkColorArg::Default,
            note: None,
        }))
        .unwrap();
    }

    app.execute_command(Command::Bookmark(BookmarkCommand::Note {
        selector: "b".to_owned(),
        note: Some("middle byte".to_owned()),
    }))
    .unwrap();
    assert_eq!(
        app.bookmark_state().entries[1].note.as_deref(),
        Some("middle byte")
    );

    app.cursor = 2;
    app.execute_command(Command::Bookmark(BookmarkCommand::Next))
        .unwrap();
    assert_eq!(app.cursor, 5);

    app.execute_command(Command::Bookmark(BookmarkCommand::Prev))
        .unwrap();
    assert_eq!(app.cursor, 1);

    app.execute_command(Command::Bookmark(BookmarkCommand::Goto {
        selector: "c".to_owned(),
    }))
    .unwrap();
    assert_eq!(app.cursor, 8);

    app.execute_command(Command::Bookmark(BookmarkCommand::Delete {
        selector: "b".to_owned(),
    }))
    .unwrap();
    assert_eq!(app.bookmark_state().entries.len(), 2);
    assert!(app.bookmark_state().find_index("b").is_none());
}

#[test]
fn bookmark_panel_click_selects_and_jumps() {
    let mut app = app_with_bytes(b"0123456789abcdef");
    for (name, start) in [("a", 1), ("b", 8)] {
        app.execute_command(Command::Bookmark(BookmarkCommand::Add {
            name: Some(name.to_owned()),
            start: Some(start),
            len: Some(1),
            color: BookmarkColorArg::Default,
            note: None,
        }))
        .unwrap();
    }
    app.view_rows = 12;
    app.cursor = 0;
    app.bookmark_state_mut().selected_row = 0;
    app.bookmark_state_mut().scroll_offset = 0;
    app.last_columns = Some(crate::view::layout::MainColumns {
        main_pane_kind: crate::view::layout::MainPaneKind::Hex,
        gutter: Rect::new(0, 0, 10, 12),
        sep1: Rect::new(10, 0, 1, 12),
        hex: Rect::new(11, 0, 48, 12),
        sep2: Rect::new(59, 0, 1, 12),
        ascii: Rect::new(60, 0, 16, 12),
        side_panel_sep: Some(Rect::new(76, 0, 1, 12)),
        side_panel: Some(Rect::new(77, 0, 40, 12)),
    });

    assert!(app.show_side_panel);
    assert_eq!(app.active_side_panel, SidePanelKind::Bookmarks);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 78,
        row: 2,
        modifiers: KeyModifiers::empty(),
    });

    assert_eq!(app.bookmark_state().selected_row, 1);
    assert_eq!(app.cursor, 8);
}

#[test]
fn bookmark_rejects_duplicate_name_and_out_of_range() {
    let mut app = app_with_bytes(b"abc");
    app.execute_command(Command::Bookmark(BookmarkCommand::Add {
        name: Some("dup".to_owned()),
        start: Some(1),
        len: Some(1),
        color: BookmarkColorArg::Default,
        note: None,
    }))
    .unwrap();

    let duplicate = app
        .execute_command(Command::Bookmark(BookmarkCommand::Add {
            name: Some("dup".to_owned()),
            start: Some(2),
            len: Some(1),
            color: BookmarkColorArg::Default,
            note: None,
        }))
        .unwrap_err();
    assert!(duplicate.to_string().contains("already exists"));

    let out_of_range = app
        .execute_command(Command::Bookmark(BookmarkCommand::Add {
            name: Some("bad".to_owned()),
            start: Some(2),
            len: Some(2),
            color: BookmarkColorArg::Default,
            note: None,
        }))
        .unwrap_err();
    assert!(out_of_range.to_string().contains("outside"));
}

#[test]
fn bookmark_selectors_distinguish_numeric_names_from_ids() {
    let mut app = app_with_bytes(b"0123456789");
    for (name, start) in [("7", 2), ("other", 8)] {
        app.execute_command(Command::Bookmark(BookmarkCommand::Add {
            name: Some(name.to_owned()),
            start: Some(start),
            len: Some(1),
            color: BookmarkColorArg::Default,
            note: None,
        }))
        .unwrap();
    }

    app.execute_command(Command::Bookmark(BookmarkCommand::Goto {
        selector: "7".to_owned(),
    }))
    .unwrap();
    assert_eq!(app.cursor, 2);

    app.execute_command(Command::Bookmark(BookmarkCommand::Goto {
        selector: "#2".to_owned(),
    }))
    .unwrap();
    assert_eq!(app.cursor, 8);
    assert_eq!(app.bookmark_state().selected_entry().unwrap().name, "other");
}

#[test]
fn generated_bookmark_name_skips_existing_name() {
    let mut app = app_with_bytes(b"abcd");
    app.execute_command(Command::Bookmark(BookmarkCommand::Add {
        name: Some("mark_2".to_owned()),
        start: Some(0),
        len: Some(1),
        color: BookmarkColorArg::Default,
        note: None,
    }))
    .unwrap();
    app.execute_command(Command::Bookmark(BookmarkCommand::Add {
        name: None,
        start: Some(1),
        len: Some(1),
        color: BookmarkColorArg::Default,
        note: None,
    }))
    .unwrap();

    assert!(app
        .bookmark_state()
        .entries
        .iter()
        .any(|entry| entry.name == "mark_3"));
}

#[test]
fn bookmark_commands_do_not_edit_document_or_undo_history() {
    let mut app = app_with_bytes(b"abcdef");
    let before = app.document.logical_bytes(0, 5).unwrap();
    let revision = app.document_revision;

    app.execute_command(Command::Bookmark(BookmarkCommand::Add {
        name: Some("annotation".to_owned()),
        start: Some(1),
        len: Some(3),
        color: BookmarkColorArg::Cyan,
        note: Some("metadata only".to_owned()),
    }))
    .unwrap();
    app.execute_command(Command::Bookmark(BookmarkCommand::Note {
        selector: "annotation".to_owned(),
        note: None,
    }))
    .unwrap();

    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), before);
    assert!(app.undo_stack.is_empty());
    assert!(app.redo_stack.is_empty());
    assert_eq!(app.document_revision, revision);
}

#[test]
fn bookmark_panel_keyboard_navigation_scroll_and_delete_work() {
    let mut app = app_with_bytes(b"0123456789abcdef");
    for (name, start, note) in [
        ("a", 1, None),
        (
            "b",
            8,
            Some("a long bookmark comment that wraps over several detail lines"),
        ),
    ] {
        app.execute_command(Command::Bookmark(BookmarkCommand::Add {
            name: Some(name.to_owned()),
            start: Some(start),
            len: Some(1),
            color: BookmarkColorArg::Blue,
            note: note.map(str::to_owned),
        }))
        .unwrap();
    }
    app.view_rows = 8;
    app.last_columns = Some(crate::view::layout::MainColumns {
        main_pane_kind: crate::view::layout::MainPaneKind::Hex,
        gutter: Rect::new(0, 0, 10, 8),
        sep1: Rect::new(10, 0, 1, 8),
        hex: Rect::new(11, 0, 30, 8),
        sep2: Rect::new(41, 0, 1, 8),
        ascii: Rect::new(42, 0, 12, 8),
        side_panel_sep: Some(Rect::new(54, 0, 1, 8)),
        side_panel: Some(Rect::new(55, 0, 20, 8)),
    });

    app.handle_action(Action::SidePanelHome);
    assert_eq!(app.bookmark_state().selected_row, 0);
    app.handle_action(Action::SidePanelEnd);
    assert_eq!(app.bookmark_state().selected_row, 1);
    app.handle_action(Action::SidePanelRight);
    assert_eq!(app.bookmark_state().detail_scroll_offset, 1);
    app.handle_action(Action::SidePanelLeft);
    assert_eq!(app.bookmark_state().detail_scroll_offset, 0);

    app.handle_action(Action::SidePanelDelete);
    assert_eq!(app.bookmark_state().entries.len(), 1);
    assert_eq!(app.bookmark_state().entries[0].name, "a");
}

#[test]
fn empty_bookmark_panel_survives_visibility_toggle() {
    let mut app = app_with_bytes(b"abc");
    app.execute_command(Command::Bookmark(BookmarkCommand::Panel))
        .unwrap();
    app.toggle_side_panel();
    assert!(!app.show_side_panel);

    app.toggle_side_panel();
    assert!(app.show_side_panel);
    assert_eq!(app.active_side_panel, SidePanelKind::Bookmarks);
    assert_eq!(app.mode, Mode::SidePanel);
}

#[test]
fn bookmark_panel_switch_closes_diff_projection() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    std::fs::write(&other, b"axc").unwrap();
    let mut app = app_with_bytes(b"abc");
    app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();
    assert!(app.diff_state().is_some());

    app.execute_command(Command::Bookmark(BookmarkCommand::Panel))
        .unwrap();

    assert!(app.diff_state().is_none());
    assert_eq!(app.active_side_panel, SidePanelKind::Bookmarks);
}
