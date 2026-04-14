use std::fs;

use tempfile::tempdir;

use crate::app::{App, SearchDirection};
use crate::cli::Cli;
use crate::commands::types::Command;
use crate::core::document::ByteSlot;
use crate::mode::{Mode, NibblePhase};

fn app_with_len(len: usize) -> App {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, vec![0_u8; len]).unwrap();
    let cli = Cli {
        file,
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
    };
    let mut app = App::from_cli(cli).unwrap();
    app.view_rows = 4;
    app
}

fn app_with_bytes(bytes: &[u8]) -> App {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, bytes).unwrap();
    let cli = Cli {
        file,
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
    };
    let mut app = App::from_cli(cli).unwrap();
    app.view_rows = 4;
    app
}

#[test]
fn scroll_viewport_moves_top_down() {
    let mut app = app_with_len(256);
    app.scroll_viewport(3);
    assert_eq!(app.viewport_top, 48);
}

#[test]
fn scroll_viewport_clamps_cursor_into_visible_range() {
    let mut app = app_with_len(256);
    app.cursor = 0;
    app.scroll_viewport(3);
    assert_eq!(app.cursor, 48);
}

#[test]
fn scroll_viewport_stops_at_last_page() {
    let mut app = app_with_len(256);
    app.scroll_viewport(99);
    assert_eq!(app.viewport_top, 192);
}

#[test]
fn edit_mode_undo_restores_previous_nibble_state() {
    let mut app = app_with_len(16);
    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };

    app.edit_nibble(0xa).unwrap();
    assert_eq!(app.cursor, 0);
    assert_eq!(
        app.mode,
        Mode::EditHex {
            phase: NibblePhase::Low
        }
    );

    app.undo(1, true).unwrap();
    assert_eq!(app.cursor, 0);
    assert_eq!(
        app.mode,
        Mode::EditHex {
            phase: NibblePhase::High
        }
    );
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0));
}

#[test]
fn command_undo_can_rewind_multiple_changes() {
    let mut app = app_with_len(16);
    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app.edit_nibble(0xa).unwrap();
    app.edit_nibble(0xb).unwrap();
    app.mode = Mode::Normal;

    app.execute_command(Command::Undo { steps: 2 }).unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.cursor, 0);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0));
}

#[test]
fn toggling_visual_tracks_selection_range() {
    let mut app = app_with_len(32);
    app.toggle_visual().unwrap();
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.selection_range(), Some((0, 0)));

    app.move_horizontal(3).unwrap();
    assert_eq!(app.selection_range(), Some((0, 3)));

    app.toggle_visual().unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_range(), None);
}

#[test]
fn visual_delete_removes_range_as_one_action() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.toggle_visual().unwrap();
    app.move_horizontal(2).unwrap();
    app.delete_at_cursor_or_selection().unwrap();

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.cursor, 0);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0x13));

    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
}

#[test]
fn search_next_and_prev_follow_last_pattern() {
    let mut app = app_with_bytes(b"abc hello xyz hello end");
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 4);

    app.repeat_search(SearchDirection::Forward).unwrap();
    assert_eq!(app.cursor, 14);

    app.repeat_search(SearchDirection::Backward).unwrap();
    assert_eq!(app.cursor, 4);
}

#[test]
fn reverse_search_command_searches_upward() {
    let mut app = app_with_bytes(b"abc hello xyz hello end");
    app.cursor = app.document.len() - 1;
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();

    assert_eq!(app.cursor, 14);

    app.repeat_search(SearchDirection::Backward).unwrap();
    assert_eq!(app.cursor, 4);
}

#[test]
fn paste_overwrites_and_appends_past_eof() {
    let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
    app.cursor = 1;
    assert_eq!(app.apply_paste_bytes(&[0xaa, 0xbb, 0xcc]).unwrap(), 3);
    assert_eq!(app.document.len(), 4);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0xcc));
}

#[test]
fn undo_reverts_entire_paste_as_one_action() {
    let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
    app.cursor = 1;
    app.apply_paste_bytes(&[0xaa, 0xbb, 0xcc]).unwrap();
    app.undo(1, true).unwrap();

    assert_eq!(app.document.len(), 3);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x22));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x33));
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Empty);
}

#[test]
fn edit_mode_can_append_at_eof() {
    let mut app = app_with_bytes(&[0x11]);
    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app.cursor = 1;
    app.edit_nibble(0xa).unwrap();
    app.edit_nibble(0xb).unwrap();
    assert_eq!(app.document.len(), 2);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));
}
