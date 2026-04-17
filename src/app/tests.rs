use std::fs;

use tempfile::tempdir;

use crate::app::{App, SearchDirection, StatusLevel};
use crate::cli::Cli;
use crate::commands::types::{Command, ExportFormat, GotoTarget, HashAlgorithm};
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
        inspector: false,
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
        inspector: false,
    };
    let mut app = App::from_cli(cli).unwrap();
    app.view_rows = 4;
    app
}

#[test]
fn app_falls_back_to_readonly_when_write_open_is_denied() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("readonly.bin");
    fs::write(&file, [0x11_u8, 0x22]).unwrap();

    let original_perms = fs::metadata(&file).unwrap().permissions();
    let mut readonly_perms = original_perms.clone();
    readonly_perms.set_readonly(true);
    fs::set_permissions(&file, readonly_perms).unwrap();

    let cli = Cli {
        file: file.clone(),
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
    };
    let app = App::from_cli(cli).unwrap();

    assert!(app.document.is_readonly());
    assert_eq!(app.status_level, StatusLevel::Warning);
    assert!(app.status_message.contains("opened read-only"));

    drop(app);
    fs::set_permissions(&file, original_perms).unwrap();
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
fn readonly_mode_allows_save_as_new_path() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, [0x11_u8, 0x22]).unwrap();

    let cli = Cli {
        file,
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        profile: false,
        readonly: true,
        no_color: true,
        offset: None,
        inspector: false,
    };
    let mut app = App::from_cli(cli).unwrap();
    let target = dir.path().join("copy.bin");

    app.execute_command(Command::Write {
        path: Some(target.clone()),
    })
    .expect("readonly save-as should succeed");

    assert_eq!(fs::read(&target).unwrap(), [0x11_u8, 0x22]);
    assert!(app.document.is_readonly());
}

#[test]
fn readonly_mode_rejects_save_in_place() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, [0x11_u8, 0x22]).unwrap();

    let cli = Cli {
        file,
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        profile: false,
        readonly: true,
        no_color: true,
        offset: None,
        inspector: false,
    };
    let mut app = App::from_cli(cli).unwrap();

    let err = app
        .execute_command(Command::Write { path: None })
        .expect_err("readonly in-place save should fail");

    assert_eq!(err.to_string(), "document is read-only");
}

#[test]
fn command_undo_clamps_eof_cursor_back_into_normal_bounds() {
    let mut app = app_with_bytes(&[0x11]);
    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app.cursor = 1;
    app.edit_nibble(0xa).unwrap();
    app.edit_nibble(0xb).unwrap();
    app.mode = Mode::Normal;

    app.execute_command(Command::Undo { steps: 2 }).unwrap();

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.document.len(), 1);
    assert_eq!(app.cursor, 0);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));
}

#[test]
fn toggling_visual_tracks_selection_range() {
    let mut app = app_with_len(32);
    app.toggle_visual();
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.selection_range(), Some((0, 0)));

    app.move_horizontal(3);
    assert_eq!(app.selection_range(), Some((0, 3)));

    app.toggle_visual();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_range(), None);
}

#[test]
fn visual_delete_removes_range_as_one_action() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.toggle_visual();
    app.move_horizontal(2);
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
fn forward_search_wraps_to_start() {
    let mut app = app_with_bytes(b"hello world");
    app.cursor = app.document.len() - 1;
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();

    assert_eq!(app.cursor, 0);
    assert!(app.status_message.contains("wrapped"));
    assert_eq!(app.status_level, StatusLevel::Notice);
}

#[test]
fn backward_search_wraps_to_end() {
    let mut app = app_with_bytes(b"hello world hello");
    app.cursor = 0;
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();

    assert_eq!(app.cursor, 12);
    assert!(app.status_message.contains("wrapped"));
    assert_eq!(app.status_level, StatusLevel::Notice);
}

#[test]
fn goto_command_supports_end_and_relative_offsets() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);

    app.execute_command(Command::Goto {
        target: GotoTarget::End,
    })
    .unwrap();
    assert_eq!(app.cursor, 3);

    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(-2),
    })
    .unwrap();
    assert_eq!(app.cursor, 1);

    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(2),
    })
    .unwrap();
    assert_eq!(app.cursor, 3);
}

#[test]
fn paste_overwrite_replaces_existing_bytes_in_place() {
    let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
    app.cursor = 1;
    assert_eq!(app.apply_paste_overwrite(&[0xaa, 0xbb]).unwrap(), 2);
    assert_eq!(app.document.len(), 3);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
}

#[test]
fn fill_command_repeats_pattern_from_cursor() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13, 0x14]);
    app.cursor = 1;
    app.execute_command(Command::Fill {
        pattern: vec![0xaa, 0xbb],
        len: 3,
    })
    .unwrap();

    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0xaa));
    assert!(app.status_message.contains("filled 3 bytes"));

    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0x13));
}

#[test]
fn export_command_writes_logical_selection_to_file() {
    let mut app = app_with_bytes(b"abcd");
    app.cursor = 1;
    app.delete_current().unwrap();
    app.cursor = 0;
    app.toggle_visual();
    app.move_horizontal(2);

    let dir = tempdir().unwrap();
    let path = dir.path().join("selection.bin");
    app.execute_command(Command::Export {
        format: ExportFormat::Binary { path: path.clone() },
    })
    .unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"ac");
    assert!(app.status_message.contains("logical bytes"));
}

#[test]
fn undo_reverts_overwrite_paste_as_one_action() {
    let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
    app.cursor = 1;
    app.apply_paste_overwrite(&[0xaa, 0xbb]).unwrap();
    app.undo(1, true).unwrap();

    assert_eq!(app.document.len(), 3);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x22));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x33));
}

#[test]
fn redo_reapplies_overwrite_paste() {
    let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
    app.cursor = 1;
    app.apply_paste_overwrite(&[0xaa, 0xbb]).unwrap();
    app.undo(1, true).unwrap();
    app.redo(1, true).unwrap();

    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
}

#[test]
fn redo_reapplies_insert_paste() {
    let mut app = app_with_bytes(&[0x11, 0x22]);
    app.cursor = 1;
    app.apply_paste_insert(&[0xaa, 0xbb]).unwrap();
    app.undo(1, true).unwrap();
    app.redo(1, true).unwrap();

    assert_eq!(app.document.len(), 4);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
}

#[test]
fn redo_reapplies_visual_delete() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.toggle_visual();
    app.move_horizontal(2);
    app.delete_at_cursor_or_selection().unwrap();
    app.undo(1, true).unwrap();
    app.redo(1, true).unwrap();

    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn command_redo_replays_undone_changes() {
    let mut app = app_with_len(16);
    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app.edit_nibble(0xa).unwrap();
    app.edit_nibble(0xb).unwrap();
    app.mode = Mode::Normal;

    app.execute_command(Command::Undo { steps: 2 }).unwrap();
    app.execute_command(Command::Redo { steps: 2 }).unwrap();

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.cursor, 1);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0xab));
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

#[test]
fn hash_command_computes_sha256_of_entire_file() {
    let mut app = app_with_bytes(b"hello");
    app.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app.status_message.contains("sha256"));
    assert!(app.status_message.contains("entire file"));
    assert!(app.status_message.contains("2cf24dba5fb0a30e"));
}

#[test]
fn hash_command_computes_crc32() {
    let mut app = app_with_bytes(b"hello");
    app.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Crc32,
    })
    .unwrap();
    assert!(app.status_message.contains("crc32"));
}

#[test]
fn hash_command_on_visual_selection_uses_selection_range() {
    let mut app = app_with_bytes(b"hello world");
    app.toggle_visual();
    app.move_horizontal(4);
    app.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Md5,
    })
    .unwrap();
    assert!(app.status_message.contains("md5"));
    assert!(app.status_message.contains("sel 0x"));
}

#[test]
fn hash_command_on_empty_file_reports_no_data() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("empty.bin");
    fs::write(&file, []).unwrap();
    let cli = Cli {
        file,
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
    };
    let mut app = App::from_cli(cli).unwrap();
    app.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app.status_message.contains("no data"));
}
