use super::*;

// ═══════════════════════════════════════════════════════════════════════════
// App initialization and readonly mode
// ═══════════════════════════════════════════════════════════════════════════

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
        file: Some(file.clone()),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
        script: Vec::new(),
    };
    let app = App::from_cli(cli).unwrap();

    assert!(app.document.is_readonly());
    assert_eq!(app.status_level, StatusLevel::Warning);
    assert!(app.status_message.contains("opened read-only"));

    drop(app);
    fs::set_permissions(&file, original_perms).unwrap();
}

#[test]
fn readonly_mode_allows_save_as_new_path() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, [0x11_u8, 0x22]).unwrap();

    let cli = Cli {
        file: Some(file),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: true,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
        script: Vec::new(),
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
        file: Some(file),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: true,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
        script: Vec::new(),
    };
    let mut app = App::from_cli(cli).unwrap();

    let err = app
        .execute_command(Command::Write { path: None })
        .expect_err("readonly in-place save should fail");

    assert_eq!(err.to_string(), "document is read-only");
}

// ═══════════════════════════════════════════════════════════════════════════
// Scroll and viewport
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scroll_viewport_operations() {
    let mut app = app_with_len(256);
    app.viewport_top = 0;
    app.scroll_viewport(3);
    assert_eq!(app.viewport_top, 48);

    // Clamps cursor into visible range
    app.cursor = 0;
    app.scroll_viewport(3);
    assert_eq!(app.cursor, 96);

    // Allows the tail row to become the top row, so large-file tail offsets can
    // be inspected directly instead of being capped at the last full page.
    app.scroll_viewport(99);
    assert_eq!(app.viewport_top, 240);
}

#[test]
fn viewport_can_scroll_to_large_tail_row() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("large.bin");
    let sparse = fs::File::create(&file).unwrap();
    sparse.set_len(0x4000_0001).unwrap();
    let cli = Cli {
        file: Some(file),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
        script: Vec::new(),
    };
    let mut app = App::from_cli(cli).unwrap();
    app.view_rows = 4;

    app.scroll_viewport(i64::MAX);

    assert_eq!(app.viewport_top, 0x4000_0000);
}

#[test]
fn diff_projection_can_scroll_to_other_only_tail_row() {
    let dir = tempdir().unwrap();
    let current = dir.path().join("current.bin");
    let other = dir.path().join("other.bin");
    let current_file = fs::File::create(&current).unwrap();
    current_file.set_len(0x4000_0000).unwrap();
    let mut other_file = fs::File::create(&other).unwrap();
    other_file.set_len(0x4000_0001).unwrap();
    other_file.seek(SeekFrom::Start(0x4000_0000)).unwrap();
    other_file.write_all(&[1]).unwrap();

    let cli = Cli {
        file: Some(current),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
        script: Vec::new(),
    };
    let mut app = App::from_cli(cli).unwrap();
    app.view_rows = 4;
    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other,
        max_shift: Some(0),
    }))
    .unwrap();

    app.scroll_viewport(i64::MAX);

    assert_eq!(app.viewport_top, 0x4000_0000);
    assert_eq!(app.cursor, 0x3fff_ffff);
}

// ═══════════════════════════════════════════════════════════════════════════
// Inspector: sync, jump, and pagination
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn inspector_field_highlight_only_when_inspector_panel_is_active() {
    let mut app = app_with_inspector_field(b"hello world", 6, 5);

    assert_eq!(app.inspector_highlight_range(), Some((6, 10)));

    app.show_side_panel = false;
    assert_eq!(app.inspector_highlight_range(), None);

    app.show_side_panel = true;
    app.active_side_panel = crate::app::SidePanelKind::Data;
    assert_eq!(app.inspector_highlight_range(), None);

    app.active_side_panel = crate::app::SidePanelKind::Diff;
    assert_eq!(app.inspector_highlight_range(), None);

    app.active_side_panel = crate::app::SidePanelKind::Inspector;
    assert_eq!(app.inspector_highlight_range(), Some((6, 10)));
}

#[test]
fn inspector_sync_and_pagination() {
    // Jump centers target row in hex view
    let bytes = vec![0_u8; 256];
    let mut app = app_with_inspector_field(&bytes, 160, 1);
    app.cursor = 0;
    app.viewport_top = 0;
    app.sync_cursor_to_inspector();
    assert_eq!(app.cursor, 160);
    assert_eq!(app.viewport_top, 128);

    // Keeps viewport when target is already visible
    app.viewport_top = 128;
    app.sync_cursor_to_inspector();
    assert_eq!(app.cursor, 160);
    assert_eq!(app.viewport_top, 128);

    // More detects nested ELF pagination markers
    let mut app2 = app_with_bytes(&build_paginated_elf64(70));
    app2.show_side_panel = true;
    app2.inspector_format_override = Some("elf".to_owned());
    app2.inspector_entry_cap = 1;
    app2.refresh_inspector();
    app2.execute_command(Command::InspectorMore).unwrap();
    assert_eq!(app2.status_level, StatusLevel::Info);
    assert!(app2.status_message.contains("more entries still pending"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Edit mode: nibble editing, undo, redo, EOF append
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edit_mode_nibble_undo_redo_and_eof_append() {
    // Undo restores previous nibble state
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
    assert_eq!(
        app.mode,
        Mode::EditHex {
            phase: NibblePhase::High
        }
    );
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0));

    // Command undo can rewind multiple changes
    let mut app2 = app_with_len(16);
    app2.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app2.edit_nibble(0xa).unwrap();
    app2.edit_nibble(0xb).unwrap();
    app2.mode = Mode::Normal;
    app2.execute_command(Command::Undo { steps: 2 }).unwrap();
    assert_eq!(app2.document.byte_at(0).unwrap(), ByteSlot::Present(0));

    // Command undo clamps EOF cursor back into normal bounds
    let mut app3 = app_with_bytes(&[0x11]);
    app3.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app3.cursor = 1;
    app3.edit_nibble(0xa).unwrap();
    app3.edit_nibble(0xb).unwrap();
    app3.mode = Mode::Normal;
    app3.execute_command(Command::Undo { steps: 2 }).unwrap();
    assert_eq!(app3.document.len(), 1);
    assert_eq!(app3.cursor, 0);
    assert_eq!(app3.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));

    // Command redo replays undone changes
    let mut app4 = app_with_len(16);
    app4.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app4.edit_nibble(0xa).unwrap();
    app4.edit_nibble(0xb).unwrap();
    app4.mode = Mode::Normal;
    app4.execute_command(Command::Undo { steps: 2 }).unwrap();
    app4.execute_command(Command::Redo { steps: 2 }).unwrap();
    assert_eq!(app4.cursor, 1);
    assert_eq!(app4.document.byte_at(0).unwrap(), ByteSlot::Present(0xab));

    // Edit mode can append at EOF
    let mut app5 = app_with_bytes(&[0x11]);
    app5.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app5.cursor = 1;
    app5.edit_nibble(0xa).unwrap();
    app5.edit_nibble(0xb).unwrap();
    assert_eq!(app5.document.len(), 2);
    assert_eq!(app5.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));
}

#[test]
fn edit_mode_noop_on_last_byte_does_not_push_insert_undo() {
    let mut app = app_with_bytes(&[0xa1]);
    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };

    app.edit_nibble(0xa).unwrap();
    app.undo(1, true).unwrap();

    assert_eq!(app.document.len(), 1);
    assert_eq!(app.cursor, 0);
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0xa1));
    assert_eq!(
        app.mode,
        Mode::EditHex {
            phase: NibblePhase::Low
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Visual mode: toggle, selection tracking, delete
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn visual_mode_selection_and_delete() {
    let mut app = app_with_len(32);
    app.toggle_visual();
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.selection_range(), Some((0, 0)));

    app.move_horizontal(3);
    assert_eq!(app.selection_range(), Some((0, 3)));

    app.toggle_visual();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_range(), None);

    // Visual delete removes range as one action
    let mut app2 = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app2.toggle_visual();
    app2.move_horizontal(2);
    app2.delete_at_cursor_or_selection().unwrap();
    assert_eq!(app2.cursor, 0);
    assert_eq!(app2.document.byte_at(0).unwrap(), ByteSlot::Deleted);
    assert_eq!(app2.document.byte_at(1).unwrap(), ByteSlot::Deleted);
    assert_eq!(app2.document.byte_at(2).unwrap(), ByteSlot::Deleted);
    assert_eq!(app2.document.byte_at(3).unwrap(), ByteSlot::Present(0x13));

    app2.undo(1, true).unwrap();
    assert_eq!(app2.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
    assert_eq!(app2.document.byte_at(1).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app2.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
}

#[test]
fn data_panel_syncs_cursor_and_mouse_selection() {
    let mut app = app_with_bytes(b"A\xce\xbb\x34\x12\x00\x00\x00");
    app.execute_command(Command::Data).unwrap();
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Data);
    assert!(app.data_state().is_some());
    assert_eq!(app.data_state().unwrap().base_offset, 0);

    app.move_horizontal(1);
    app.handle_action(Action::MoveRight);
    assert_eq!(app.cursor, 2);
    assert_eq!(app.data_state().unwrap().base_offset, 2);

    app.last_columns = Some(crate::view::layout::MainColumns {
        main_pane_kind: crate::view::layout::MainPaneKind::Hex,
        gutter: ratatui::layout::Rect::new(0, 0, 8, 8),
        sep1: ratatui::layout::Rect::new(8, 0, 1, 8),
        hex: ratatui::layout::Rect::new(9, 0, 20, 8),
        sep2: ratatui::layout::Rect::new(29, 0, 1, 8),
        ascii: ratatui::layout::Rect::new(30, 0, 20, 8),
        side_panel_sep: Some(ratatui::layout::Rect::new(50, 0, 1, 8)),
        side_panel: Some(ratatui::layout::Rect::new(51, 0, 40, 8)),
    });
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 52,
        row: 5,
        modifiers: crossterm::event::KeyModifiers::empty(),
    });

    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.selection_range(), Some((2, 3)));
    assert_eq!(
        app.data_state().unwrap().selected_label.as_deref(),
        Some("uint16")
    );
}
