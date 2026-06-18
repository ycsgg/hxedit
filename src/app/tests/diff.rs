use super::*;

#[test]
fn diff_command_opens_synced_page_without_full_scan() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    fs::write(&other, b"abXYcd").unwrap();
    let mut app = app_with_bytes(b"abcd");

    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other.clone(),
        max_shift: Some(2),
    }))
    .unwrap();

    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Diff);
    assert!(app.show_side_panel);
    assert_eq!(app.mode, Mode::SidePanel);
    let diff = app.diff_state().unwrap();
    assert_eq!(diff.options.max_shift, 2);
    assert_eq!(diff.other_len, 6);
    assert!(!diff.stale);
    assert!(app.status_message.contains("logical bytes"));

    app.mode = Mode::Normal;
    app.handle_action(Action::DeleteByte);
    assert!(!app.diff_state().unwrap().stale);

    app.execute_command(Command::Diff(DiffCommand::Refresh))
        .unwrap();
    assert!(!app.diff_state().unwrap().stale);
}

#[test]
fn diff_navigation_and_off_behaviour() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    fs::write(&other, b"axcd").unwrap();
    let mut app = app_with_bytes(b"abcd");
    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other,
        max_shift: Some(0),
    }))
    .unwrap();

    app.execute_command(Command::Diff(DiffCommand::Next))
        .unwrap();
    assert_eq!(app.cursor, 1);

    app.execute_command(Command::Diff(DiffCommand::Off))
        .unwrap();
    assert!(app.diff_state().is_none());
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Inspector);
    assert!(!app.show_side_panel);
}

#[test]
fn diff_next_finds_far_mismatch_with_chunk_scan() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    let current = vec![0_u8; 2 * 1024 * 1024];
    let mut other_bytes = current.clone();
    *other_bytes.last_mut().unwrap() = 1;
    fs::write(&other, other_bytes).unwrap();

    let mut app = app_with_bytes(&current);
    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other,
        max_shift: Some(0),
    }))
    .unwrap();

    app.cursor = 0;
    app.execute_command(Command::Diff(DiffCommand::Next))
        .unwrap();

    assert_eq!(app.cursor, current.len() as u64 - 1);
}

#[test]
fn diff_next_large_scan_steps_across_ticks() {
    let dir = tempdir().unwrap();
    let current = dir.path().join("current.bin");
    let other = dir.path().join("other.bin");
    let size = 129 * 1024 * 1024_u64;

    let current_file = fs::File::create(&current).unwrap();
    current_file.set_len(size).unwrap();
    let mut other_file = fs::File::create(&other).unwrap();
    other_file.set_len(size).unwrap();
    other_file.seek(SeekFrom::Start(size - 1)).unwrap();
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
    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other,
        max_shift: Some(0),
    }))
    .unwrap();

    app.cursor = 0;
    app.execute_command(Command::Diff(DiffCommand::Next))
        .unwrap();

    assert!(app.diff_mismatch_scan_pending());
    assert_eq!(app.cursor, 0);
    assert!(app.status_message.contains("diff scanning next"));

    let mut steps = 0;
    while app.diff_mismatch_scan_pending() {
        app.continue_diff_mismatch_scan().unwrap();
        steps += 1;
        assert!(steps <= 1);
    }

    assert_eq!(app.cursor, size - 1);
    assert!(app.status_message.contains("diff mismatch"));
}

#[test]
fn diff_scan_blocks_navigation_until_escape() {
    let dir = tempdir().unwrap();
    let current = dir.path().join("current.bin");
    let other = dir.path().join("other.bin");
    let size = 160 * 1024 * 1024_u64;

    let current_file = fs::File::create(&current).unwrap();
    current_file.set_len(size).unwrap();
    let mut other_file = fs::File::create(&other).unwrap();
    other_file.set_len(size).unwrap();
    other_file.seek(SeekFrom::Start(size - 1)).unwrap();
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
    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other,
        max_shift: Some(0),
    }))
    .unwrap();

    app.cursor = 0;
    app.execute_command(Command::Diff(DiffCommand::Next))
        .unwrap();
    assert!(app.diff_mismatch_scan_pending());

    app.handle_action(Action::MoveDown);
    assert_eq!(app.cursor, 0);
    assert!(app.diff_mismatch_scan_pending());

    app.handle_action(Action::LeaveMode);
    assert_eq!(app.cursor, 0);
    assert!(!app.diff_mismatch_scan_pending());
    assert!(app.status_message.contains("diff scan canceled"));
}

#[test]
fn diff_closes_when_switching_or_hiding_side_panel() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    fs::write(&other, b"abXc").unwrap();

    let mut app = app_with_bytes(b"abc");
    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other.clone(),
        max_shift: None,
    }))
    .unwrap();
    app.handle_action(Action::ToggleSidePanel);
    assert!(app.diff_state().is_none());
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Inspector);
    assert!(!app.show_side_panel);

    let mut app2 = app_with_bytes(b"abc");
    app2.execute_command(Command::Diff(DiffCommand::Open {
        path: other.clone(),
        max_shift: None,
    }))
    .unwrap();
    app2.execute_command(Command::Data).unwrap();
    assert!(app2.diff_state().is_none());
    assert_eq!(app2.active_side_panel, crate::app::SidePanelKind::Data);
    assert!(app2.data_state().is_some());

    let mut app3 = app_with_bytes(b"abc");
    app3.inspector_error = Some("manual inspector placeholder".to_owned());
    app3.execute_command(Command::Diff(DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();
    app3.execute_command(Command::Inspector).unwrap();
    assert!(app3.diff_state().is_none());
    assert_eq!(app3.active_side_panel, crate::app::SidePanelKind::Inspector);
}

#[test]
fn diff_mouse_selection_counts_projected_placeholders_and_right_side() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    let base = (0..0x140)
        .map(|idx| (idx as u8).wrapping_mul(37).wrapping_add(11))
        .collect::<Vec<_>>();
    let mut other_bytes = base.clone();
    other_bytes.insert(0xba, 0xab);
    fs::write(&other, &other_bytes).unwrap();

    let mut app = app_with_bytes(&base);
    app.viewport_top = 0xb0;
    app.view_rows = 4;
    app.execute_command(Command::Diff(DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();
    app.last_columns = Some(crate::view::layout::MainColumns {
        main_pane_kind: crate::view::layout::MainPaneKind::Hex,
        gutter: ratatui::layout::Rect::new(0, 0, 8, 4),
        sep1: ratatui::layout::Rect::new(8, 0, 1, 4),
        hex: ratatui::layout::Rect::new(9, 0, 49, 4),
        sep2: ratatui::layout::Rect::new(58, 0, 1, 4),
        ascii: ratatui::layout::Rect::new(59, 0, 17, 4),
        side_panel_sep: Some(ratatui::layout::Rect::new(76, 0, 1, 4)),
        side_panel: Some(ratatui::layout::Rect::new(77, 0, 80, 4)),
    });

    // Left-side projected `__` occupies the visual cell at row 0 / col 0x0a.
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 9 + 32,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::empty(),
    });
    assert_eq!(app.cursor, 0xba);
    assert_eq!(
        app.diff_state().unwrap().selected_other_offset,
        None,
        "left current-side click should not leave a right-side selection"
    );

    // Right-side panel click on the corresponding other-only byte synchronizes
    // the left cursor and records the selected other raw offset for highlighting.
    app.cursor = 0xb0;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 77 + 41,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::empty(),
    });
    assert_eq!(app.cursor, 0xba);
    assert_eq!(app.diff_state().unwrap().selected_other_offset, Some(0xba));
    assert_eq!(
        app.diff_state().unwrap().selected_other_anchor_display,
        Some(0xba)
    );
}
