use super::*;

#[cfg(feature = "memory")]
#[test]
fn mem_command_opens_memory_side_panel_placeholder() {
    let mut app = app_with_bytes(b"abcdef");
    app.execute_command(Command::Memory(crate::commands::types::MemoryCommand::Open))
        .unwrap();

    assert!(app.show_side_panel);
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Memory);
    assert!(matches!(app.mode, Mode::SidePanel));
    assert!(app.status_message.contains("memory panel opened"));
    assert!(app.memory_state().is_some());
}

#[cfg(feature = "memory")]
fn app_with_fake_memory(bytes: Vec<u8>) -> (App, crate::memory::FakeMemoryBackend) {
    let control = crate::memory::FakeMemoryBackend::new();
    let region = control.add_region(
        0x1000,
        bytes,
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(1),
    );
    let mut session = crate::memory::MemorySession::open(Box::new(control.clone())).unwrap();
    let document = session.document_for_region(0, &Config::default()).unwrap();
    let mut app = app_with_bytes(b"memory-placeholder");
    app.document = document;
    app.cursor = 0;
    app.viewport_top = 0;
    app.set_memory_runtime(
        super::super::memory_state::MemoryRuntime {
            session,
            selected_region: 0,
            opened_region: 0,
            base_va: region.start,
            region_edits: std::collections::HashMap::new(),
        },
        "attached to fake memory".to_owned(),
    );
    (app, control)
}

#[cfg(feature = "memory")]
#[test]
fn mem_commit_writes_replacement_spans_to_backend_and_clears_dirty_state() {
    let (mut app, control) = app_with_fake_memory(vec![1, 2, 3, 4]);

    app.document.set_byte(1, 0xaa).unwrap();
    app.document.set_byte(2, 0xbb).unwrap();
    assert!(app.document.is_dirty());

    app.execute_command(Command::Memory(
        crate::commands::types::MemoryCommand::Commit,
    ))
    .unwrap();

    assert_eq!(
        control.region_bytes(0x1000).unwrap(),
        vec![1, 0xaa, 0xbb, 4]
    );
    assert_eq!(control.write_count(), 1);
    assert!(!app.document.is_dirty());
    assert!(app.status_message.contains("memory commit wrote 2 bytes"));
    assert!(app.status_message.contains("target was running"));
    assert_eq!(
        app.memory_runtime()
            .unwrap()
            .session
            .region_dirty_bytes(0)
            .unwrap(),
        0
    );
}

#[cfg(feature = "memory")]
#[test]
fn mem_freeze_and_thaw_update_session_state() {
    let (mut app, control) = app_with_fake_memory(vec![1, 2, 3, 4]);

    app.execute_command(Command::Memory(
        crate::commands::types::MemoryCommand::Freeze,
    ))
    .unwrap();
    assert!(control.is_frozen());
    assert_eq!(control.freeze_count(), 1);
    assert!(app.memory_runtime().unwrap().session.is_frozen());
    assert!(app.status_message.contains("froze memory target"));

    app.execute_command(Command::Memory(crate::commands::types::MemoryCommand::Thaw))
        .unwrap();
    assert!(!control.is_frozen());
    assert_eq!(control.thaw_count(), 1);
    assert!(!app.memory_runtime().unwrap().session.is_frozen());
    assert!(app.status_message.contains("thawed memory target"));
}

#[cfg(feature = "memory")]
#[test]
fn mem_commit_while_frozen_omits_running_warning() {
    let (mut app, _control) = app_with_fake_memory(vec![1, 2, 3, 4]);
    app.execute_command(Command::Memory(
        crate::commands::types::MemoryCommand::Freeze,
    ))
    .unwrap();
    app.document.set_byte(1, 0xaa).unwrap();

    app.execute_command(Command::Memory(
        crate::commands::types::MemoryCommand::Commit,
    ))
    .unwrap();

    assert!(app.status_message.contains("memory commit wrote 1 byte"));
    assert!(!app.status_message.contains("target was running"));
}

#[cfg(feature = "memory")]
#[test]
fn mem_commit_all_commits_single_opened_region() {
    let (mut app, control) = app_with_fake_memory(vec![1, 2, 3, 4]);

    app.document.set_byte(0, 0x10).unwrap();
    app.document.set_byte(3, 0x40).unwrap();
    app.execute_command(Command::Memory(
        crate::commands::types::MemoryCommand::CommitAll,
    ))
    .unwrap();

    assert_eq!(
        control.region_bytes(0x1000).unwrap(),
        vec![0x10, 2, 3, 0x40]
    );
    assert_eq!(control.write_count(), 2);
    assert!(!app.document.is_dirty());
    assert!(app
        .status_message
        .contains("memory commit-all wrote 2 bytes"));
}

#[cfg(feature = "memory")]
#[test]
fn memory_goto_absolute_accepts_virtual_address() {
    let (mut app, _control) = app_with_fake_memory(vec![1, 2, 3, 4]);

    app.execute_command(Command::Goto {
        target: GotoTarget::Absolute(0x1002),
    })
    .unwrap();

    assert_eq!(app.cursor, 2);
    assert!(app.status_message.contains("VA 0x1002"));
}

#[cfg(feature = "memory")]
#[test]
fn memory_search_opens_hit_region_and_reports_virtual_address() {
    let control = crate::memory::FakeMemoryBackend::new();
    let first = control.add_region(
        0x1000,
        b"aaaa".to_vec(),
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(1),
    );
    control.add_region(
        0x2000,
        b"xxneedle".to_vec(),
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(2),
    );
    let mut session = crate::memory::MemorySession::open(Box::new(control.clone())).unwrap();
    let document = session.document_for_region(0, &Config::default()).unwrap();
    let mut app = app_with_bytes(b"memory-placeholder");
    app.document = document;
    app.set_memory_runtime(
        super::super::memory_state::MemoryRuntime {
            session,
            selected_region: 0,
            opened_region: 0,
            base_va: first.start,
            region_edits: std::collections::HashMap::new(),
        },
        "attached to fake memory".to_owned(),
    );

    app.execute_command(Command::MemorySearch {
        query: crate::memory::MemorySearchQuery::parse("/needle/").unwrap(),
        backward: false,
    })
    .unwrap();

    assert_eq!(app.memory_runtime().unwrap().selected_region, 1);
    assert_eq!(app.cursor, 2);
    assert!(app
        .document
        .path()
        .to_string_lossy()
        .contains("0x2000-0x2008"));
    assert!(app.status_message.contains("VA 0x2002"));
}

#[cfg(feature = "memory")]
#[test]
fn memory_panel_selection_does_not_change_active_document_base_va() {
    let control = crate::memory::FakeMemoryBackend::new();
    let first = control.add_region(
        0x1000,
        b"abcd".to_vec(),
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(1),
    );
    control.add_region(
        0x2000,
        b"wxyz".to_vec(),
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(2),
    );
    let mut session = crate::memory::MemorySession::open(Box::new(control)).unwrap();
    let document = session.document_for_region(0, &Config::default()).unwrap();
    let mut app = app_with_bytes(b"memory-placeholder");
    app.document = document;
    app.cursor = 1;
    app.set_memory_runtime(
        super::super::memory_state::MemoryRuntime {
            session,
            selected_region: 0,
            opened_region: 0,
            base_va: first.start,
            region_edits: std::collections::HashMap::new(),
        },
        "attached to fake memory".to_owned(),
    );

    app.move_memory_selection(1);
    assert_eq!(app.memory_runtime().unwrap().selected_region, 1);
    assert_eq!(app.display_offset_to_va(app.cursor), Some(0x1001));

    app.execute_command(Command::Goto {
        target: GotoTarget::Absolute(0x1002),
    })
    .unwrap();
    assert_eq!(app.cursor, 2);
    assert!(app.status_message.contains("VA 0x1002"));
}

#[cfg(feature = "memory")]
fn app_with_two_fake_regions() -> (App, crate::memory::FakeMemoryBackend) {
    let control = crate::memory::FakeMemoryBackend::new();
    control.add_region(
        0x1000,
        vec![1, 2, 3, 4],
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(1),
    );
    control.add_region(
        0x2000,
        vec![5, 6, 7, 8],
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(2),
    );
    let mut session = crate::memory::MemorySession::open(Box::new(control.clone())).unwrap();
    let document = session.document_for_region(0, &Config::default()).unwrap();
    let mut app = app_with_bytes(b"memory-placeholder");
    app.document = document;
    app.cursor = 0;
    app.viewport_top = 0;
    app.set_memory_runtime(
        super::super::memory_state::MemoryRuntime {
            session,
            selected_region: 0,
            opened_region: 0,
            base_va: 0x1000,
            region_edits: std::collections::HashMap::new(),
        },
        "attached to fake memory".to_owned(),
    );
    (app, control)
}

#[cfg(feature = "memory")]
#[test]
fn mem_write_with_path_is_rejected_and_points_to_export() {
    let (mut app, _control) = app_with_fake_memory(vec![1, 2, 3, 4]);
    let err = app
        .execute_command(Command::Write {
            path: Some(std::path::PathBuf::from("/tmp/out.bin")),
        })
        .unwrap_err();
    assert!(err.to_string().contains(":export"));
}

#[cfg(feature = "memory")]
#[test]
fn mem_write_without_path_commits_like_mem_commit() {
    let (mut app, control) = app_with_fake_memory(vec![1, 2, 3, 4]);
    app.document.set_byte(1, 0xaa).unwrap();

    app.execute_command(Command::Write { path: None }).unwrap();

    assert_eq!(control.region_bytes(0x1000).unwrap(), vec![1, 0xaa, 3, 4]);
    assert!(!app.document.is_dirty());
    assert!(app.status_message.contains("memory commit wrote"));
}

#[cfg(feature = "memory")]
#[test]
fn mem_per_region_edits_survive_region_switch() {
    let (mut app, _control) = app_with_two_fake_regions();

    // Edit region 0.
    app.document.set_byte(1, 0xaa).unwrap();
    assert!(app.document.is_dirty());

    // Switch to region 1, edit it.
    app.memory_runtime_mut().unwrap().selected_region = 1;
    app.open_selected_memory_region().unwrap();
    assert_eq!(app.memory_runtime().unwrap().opened_region, 1);
    assert!(!app.document.is_dirty());
    app.document.set_byte(0, 0xbb).unwrap();

    // Switch back to region 0: its replacement must still be present.
    app.memory_runtime_mut().unwrap().selected_region = 0;
    app.open_selected_memory_region().unwrap();
    assert_eq!(app.memory_runtime().unwrap().opened_region, 0);
    assert_eq!(
        app.document.replacement_spans().unwrap(),
        vec![(1, vec![0xaa])]
    );
    assert!(app.document.is_dirty());
}

#[cfg(feature = "memory")]
#[test]
fn mem_commit_all_writes_every_dirty_region_in_va_order() {
    let (mut app, control) = app_with_two_fake_regions();

    // Edit region 0, then switch to region 1 and edit it too.
    app.document.set_byte(0, 0x10).unwrap();
    app.memory_runtime_mut().unwrap().selected_region = 1;
    app.open_selected_memory_region().unwrap();
    app.document.set_byte(3, 0x80).unwrap();

    app.execute_command(Command::Memory(
        crate::commands::types::MemoryCommand::CommitAll,
    ))
    .unwrap();

    assert_eq!(control.region_bytes(0x1000).unwrap(), vec![0x10, 2, 3, 4]);
    assert_eq!(control.region_bytes(0x2000).unwrap(), vec![5, 6, 7, 0x80]);
    assert!(app.status_message.contains("commit-all wrote 2 bytes"));
    assert!(app.status_message.contains("2 regions"));
    assert!(app.memory_dirty_summary().is_none());
}

#[cfg(feature = "memory")]
#[test]
fn mem_commit_all_stops_on_non_writable_region_and_keeps_dirty() {
    let control = crate::memory::FakeMemoryBackend::new();
    control.add_region(
        0x1000,
        vec![1, 2, 3, 4],
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(1),
    );
    control.add_region(
        0x2000,
        vec![5, 6, 7, 8],
        crate::memory::MemoryPermissions::readable(),
        crate::memory::RegionFingerprint(2),
    );
    let mut session = crate::memory::MemorySession::open(Box::new(control.clone())).unwrap();
    let document = session.document_for_region(0, &Config::default()).unwrap();
    let mut app = app_with_bytes(b"memory-placeholder");
    app.document = document;
    app.set_memory_runtime(
        super::super::memory_state::MemoryRuntime {
            session,
            selected_region: 0,
            opened_region: 0,
            base_va: 0x1000,
            region_edits: std::collections::HashMap::new(),
        },
        "attached to fake memory".to_owned(),
    );

    // Stash a dirty edit on the non-writable region 1 directly.
    app.memory_runtime_mut().unwrap().region_edits.insert(
        1,
        super::super::memory_state::RegionEditState {
            spans: vec![(0, vec![0x99])],
            undo: Vec::new(),
            redo: Vec::new(),
            cursor: 0,
        },
    );
    // Dirty edit on writable region 0.
    app.document.set_byte(0, 0x10).unwrap();

    app.execute_command(Command::Memory(
        crate::commands::types::MemoryCommand::CommitAll,
    ))
    .unwrap();

    // Region 0 (lower VA) committed first; region 1 rejected, kept dirty.
    assert_eq!(control.region_bytes(0x1000).unwrap(), vec![0x10, 2, 3, 4]);
    assert_eq!(control.region_bytes(0x2000).unwrap(), vec![5, 6, 7, 8]);
    assert!(app.status_message.contains("not writable"));
    assert!(app.status_message.contains("1/2 regions committed"));
    assert_eq!(app.memory_dirty_summary(), Some((1, 1)));
}

#[cfg(feature = "memory")]
#[test]
fn mem_info_aggregates_dirty_undo_freeze_and_access() {
    let (mut app, _control) = app_with_two_fake_regions();

    // Dirty edit on opened region 0 via the edit path so undo depth is set.
    app.cursor = 0;
    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app.handle_action(Action::EditHex(0xa));
    app.handle_action(Action::EditHex(0xa));
    assert!(app.document.is_dirty());

    // Stash a dirty edit on region 1 too.
    app.memory_runtime_mut().unwrap().region_edits.insert(
        1,
        super::super::memory_state::RegionEditState {
            spans: vec![(0, vec![0x99, 0x88])],
            undo: Vec::new(),
            redo: Vec::new(),
            cursor: 0,
        },
    );

    app.execute_command(Command::Memory(crate::commands::types::MemoryCommand::Info))
        .unwrap();

    // :mem info now opens a dedicated Info panel; the full report lives in the
    // panel message (multi-line), the status line only shows the first line.
    let info = &app.memory_state().unwrap().message;
    assert!(info.contains("fp=0x1"), "info missing fingerprint: {info}");
    assert!(info.contains("access rw"), "info missing access: {info}");
    assert!(info.contains("undo 2"), "info missing undo depth: {info}");
    assert!(
        info.contains("session dirty 2 regions"),
        "info missing session totals: {info}"
    );
    assert!(
        info.contains("3 bytes total"),
        "info missing byte total: {info}"
    );
    assert!(
        info.contains("target running"),
        "info missing freeze: {info}"
    );
}

#[cfg(feature = "memory")]
#[test]
fn mem_quit_blocked_when_any_region_dirty_and_forced_quit_discards() {
    let (mut app, _control) = app_with_two_fake_regions();
    app.document.set_byte(1, 0xaa).unwrap();

    let err = app
        .execute_command(Command::Quit { force: false })
        .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("regions dirty"));
    assert!(text.contains(":q!"));
    assert!(!app.should_quit);

    app.execute_command(Command::Quit { force: true }).unwrap();
    assert!(app.should_quit);
}

#[cfg(feature = "memory")]
#[test]
fn mem_search_repeat_with_gn_advances_to_next_hit() {
    let control = crate::memory::FakeMemoryBackend::new();
    control.add_region(
        0x1000,
        b"ab__ab".to_vec(),
        crate::memory::MemoryPermissions::read_write(),
        crate::memory::RegionFingerprint(1),
    );
    let mut session = crate::memory::MemorySession::open(Box::new(control)).unwrap();
    let document = session.document_for_region(0, &Config::default()).unwrap();
    let mut app = app_with_bytes(b"memory-placeholder");
    app.document = document;
    app.set_memory_runtime(
        super::super::memory_state::MemoryRuntime {
            session,
            selected_region: 0,
            opened_region: 0,
            base_va: 0x1000,
            region_edits: std::collections::HashMap::new(),
        },
        "attached to fake memory".to_owned(),
    );

    app.execute_command(Command::MemorySearch {
        query: crate::memory::MemorySearchQuery::parse("/ab/").unwrap(),
        backward: false,
    })
    .unwrap();
    // Forward search starts after the cursor, so from offset 0 it lands on the
    // second "ab" at offset 4.
    assert_eq!(app.cursor, 4);

    // gn repeats the memory search forward, wrapping to the first "ab".
    app.repeat_memory_search(false).unwrap();
    assert_eq!(app.cursor, 0);
    assert!(app.status_message.contains("VA 0x1000"));

    // gN repeats backward to the later occurrence.
    app.repeat_memory_search(true).unwrap();
    assert_eq!(app.cursor, 4);
}

#[cfg(feature = "memory")]
#[test]
fn mem_search_repeat_without_query_reports_and_keeps_file_search_separate() {
    let (mut app, _control) = app_with_fake_memory(b"hello".to_vec());

    // No memory search yet: gn is a no-op with guidance.
    app.repeat_memory_search(false).unwrap();
    assert!(app.status_message.contains("no active memory search"));

    // A file-style byte search must not populate the memory-search history.
    app.execute_command(Command::SearchAscii {
        pattern: b"lo".to_vec(),
        backward: false,
    })
    .unwrap();
    app.repeat_memory_search(false).unwrap();
    assert!(app.status_message.contains("no active memory search"));
}

#[cfg(feature = "memory")]
#[test]
fn mem_process_list_panel_navigation_and_click_select_rows() {
    use crate::memory::ProcessInfo;

    let (mut app, _control) = app_with_fake_memory(vec![1, 2, 3, 4]);
    let processes = vec![
        ProcessInfo::new(11, "alpha"),
        ProcessInfo::new(22, "beta"),
        ProcessInfo::new(33, "gamma"),
    ];
    app.open_memory_process_list_panel(processes, "3 processes (Enter to attach)");

    assert_eq!(
        app.memory_state().unwrap().view,
        super::super::memory_state::MemoryPanelView::ProcessList
    );
    assert_eq!(app.memory_state().unwrap().selected_row, 0);

    app.move_memory_selection(2);
    assert_eq!(app.memory_state().unwrap().selected_row, 2);
    app.move_memory_selection(-1);
    assert_eq!(app.memory_state().unwrap().selected_row, 1);

    // Click maps a body row (no leading header rows in ProcessList view).
    app.handle_memory_panel_click(0);
    assert_eq!(app.memory_state().unwrap().selected_row, 0);
}

#[cfg(feature = "memory")]
#[test]
fn mem_maps_click_changes_highlight_only_not_opened_region() {
    let (mut app, _control) = app_with_two_fake_regions();
    assert_eq!(
        app.memory_state().unwrap().view,
        super::super::memory_state::MemoryPanelView::Maps
    );

    // Each region now occupies two body rows; region 1 starts after the header
    // rows plus region 0's two-line entry.
    let row = super::super::memory_state::MEMORY_MAPS_HEADER_ROWS + 2;
    app.handle_memory_panel_click(row);
    assert_eq!(app.memory_runtime().unwrap().selected_region, 1);
    // Clicking only highlights; the opened region / document is unchanged.
    assert_eq!(app.memory_runtime().unwrap().opened_region, 0);
}

#[cfg(feature = "memory")]
#[test]
fn mem_maps_scroll_clamps_and_click_uses_clamped_offset() {
    let (mut app, _control) = app_with_two_fake_regions();

    app.scroll_memory_panel(100);
    let expected_scroll = super::super::memory_state::MEMORY_MAPS_HEADER_ROWS
        + super::super::memory_state::MEMORY_MAPS_REGION_ROWS * 2
        - app.side_panel_visible_rows();
    assert_eq!(app.memory_state().unwrap().scroll_offset, expected_scroll);

    // With scroll clamped to the max, visible row 2 maps to absolute line 7:
    // region 1's summary line.
    app.handle_memory_panel_click(2);
    assert_eq!(app.memory_runtime().unwrap().selected_region, 1);
    assert_eq!(app.memory_runtime().unwrap().opened_region, 0);
}

#[cfg(feature = "memory")]
#[test]
fn mem_info_panel_scrolls_without_changing_selection() {
    let (mut app, _control) = app_with_fake_memory(vec![1, 2, 3, 4]);
    app.execute_command(Command::Memory(crate::commands::types::MemoryCommand::Info))
        .unwrap();
    assert_eq!(
        app.memory_state().unwrap().view,
        super::super::memory_state::MemoryPanelView::Info
    );

    let max_scroll = app
        .memory_state()
        .unwrap()
        .message
        .lines()
        .count()
        .saturating_sub(app.side_panel_visible_rows());
    app.scroll_memory_panel(3);
    assert_eq!(app.memory_state().unwrap().scroll_offset, max_scroll);
    app.scroll_memory_panel(-10);
    assert_eq!(app.memory_state().unwrap().scroll_offset, 0);
}

#[cfg(feature = "memory")]
#[test]
fn mem_attach_blocked_when_current_session_dirty() {
    let (mut app, _control) = app_with_two_fake_regions();
    app.document.set_byte(0, 0x10).unwrap();

    // Enter the process-list view with a candidate process.
    app.open_memory_process_list_panel(
        vec![crate::memory::ProcessInfo::new(4242, "fake")],
        "1 process",
    );
    app.handle_memory_panel_enter().unwrap();

    // Dirty guard refuses to switch; original session/document intact.
    assert!(app.status_message.contains("commit or :q!"));
    assert_eq!(app.memory_runtime().unwrap().opened_region, 0);
    assert!(app.document.is_dirty());
}

#[cfg(feature = "disasm")]
#[test]
fn disassembly_insert_blocked_and_replace_restricted() {
    let bytes = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    // Insert mode blocked
    app.handle_action(crate::action::Action::EnterInsert);
    assert!(app.status_message.contains("overwrite-only"));
    assert!(matches!(app.mode, Mode::Normal));

    // Equal length replace works
    let mut app2 = app_with_bytes(&bytes);
    app2.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app2.execute_command(Command::Replace {
        needle: vec![0x90],
        replacement: vec![0xcc],
        allow_resize: false,
        force: false,
    })
    .unwrap();
    let state2 = match &app2.main_view {
        crate::app::MainView::Disassembly(s) => s.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows2 = app2
        .collect_disassembly_rows(&state2, state2.viewport_top, 2)
        .unwrap();
    assert!(rows2[0].text.contains("int3"));

    // Resize replace blocked
    let mut app3 = app_with_bytes(&bytes);
    app3.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let result = app3.execute_command(Command::Replace {
        needle: vec![0x90],
        replacement: vec![0xcc, 0xcc],
        allow_resize: true,
        force: false,
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("overwrite-only"));
}
