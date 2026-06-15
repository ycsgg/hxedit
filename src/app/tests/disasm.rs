use super::*;

// ═══════════════════════════════════════════════════════════════════════════
// Disassembly: view switch, viewport alignment, symbols
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "disasm-capstone")]
#[test]
fn disassemble_command_switches_view_and_aligns_viewport() {
    let bytes = {
        let mut bytes = vec![0_u8; 0x200];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        let ph = 64usize;
        bytes[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[ph + 4..ph + 8].copy_from_slice(&0x5u32.to_le_bytes());
        bytes[ph + 8..ph + 16].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&4u64.to_le_bytes());
        bytes[0x100..0x104].copy_from_slice(&[0x90, 0x90, 0x90, 0xc3]);
        bytes
    };
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    assert!(matches!(
        app.main_view,
        crate::app::MainView::Disassembly(_)
    ));
    assert_eq!(app.cursor, 0x100);
    assert!(app.status_message.contains("disassembly:"));

    // Viewport alignment
    let bytes2 = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app2 = app_with_bytes(&bytes2);
    app2.cursor = 0x102;
    app2.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    assert_eq!(app2.cursor, 0x102);
    match &app2.main_view {
        crate::app::MainView::Disassembly(state) => assert_eq!(state.viewport_top, 0x101),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols"))]
#[test]
fn disassembly_symbols_and_call_targets() {
    // Symbol labels and virtual addresses
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0xc3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    assert!(app.status_message.contains("[1 syms]"));
    let state = match &app.main_view {
        crate::app::MainView::Disassembly(state) => state.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows = app
        .collect_disassembly_rows(&state, state.viewport_top, 2)
        .unwrap();
    assert_eq!(rows[0].virtual_address, Some(0x401000));
    assert_eq!(rows[0].symbol_label.as_deref(), Some("entry"));

    // Symbolizes exact immediate operands
    let bytes2 =
        build_disassembly_elf64_with_symbol(&[0xB8, 0x00, 0x10, 0x40, 0x00, 0xC3], "entry");
    let mut app2 = app_with_bytes(&bytes2);
    app2.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let state2 = match &app2.main_view {
        crate::app::MainView::Disassembly(s) => s.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows2 = app2
        .collect_disassembly_rows(&state2, state2.viewport_top, 1)
        .unwrap();
    assert!(rows2[0].text.contains("entry"));

    // Normalizes platform symbol decorations
    let bytes3 = build_disassembly_elf64_with_symbol(
        &[0xB8, 0x00, 0x10, 0x40, 0x00, 0xC3],
        "_entry@@GLIBC_2.2.5",
    );
    let mut app3 = app_with_bytes(&bytes3);
    app3.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let state3 = match &app3.main_view {
        crate::app::MainView::Disassembly(s) => s.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows3 = app3
        .collect_disassembly_rows(&state3, state3.viewport_top, 1)
        .unwrap();
    assert_eq!(rows3[0].symbol_label.as_deref(), Some("entry"));
    assert!(!rows3[0].text.contains("GLIBC"));

    // Resolves x86 direct call target
    let bytes4 =
        build_disassembly_elf64_with_symbol(&[0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xC3], "entry");
    let mut app4 = app_with_bytes(&bytes4);
    app4.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let state4 = match &app4.main_view {
        crate::app::MainView::Disassembly(s) => s.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows4 = app4
        .collect_disassembly_rows(&state4, state4.viewport_top, 3)
        .unwrap();
    let target = rows4[1].direct_target.as_ref().expect("direct target");
    assert_eq!(rows4[1].text, "call entry");
    assert_eq!(target.virtual_address, 0x401000);
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols"))]
#[test]
fn symbol_search_finds_all_symbolized_occurrences() {
    let bytes = build_disassembly_elf64_with_symbol(
        &[
            0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xE8, 0xF5, 0xFF, 0xFF, 0xFF, 0xC3,
        ],
        "entry",
    );
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.execute_command(Command::SearchSymbol {
        pattern: "entry".to_owned(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 0x101);

    app.repeat_search(SearchDirection::Forward).unwrap();
    assert_eq!(app.cursor, 0x106);

    app.repeat_search(SearchDirection::Forward).unwrap();
    assert_eq!(app.cursor, 0x100);
    assert_eq!(app.status_level, StatusLevel::Notice);

    app.repeat_search(SearchDirection::Backward).unwrap();
    assert_eq!(app.cursor, 0x106);
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols"))]
#[test]
fn symbol_panel_toggle_restores_symbol_page() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0xc3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.execute_command(Command::Symbols).unwrap();

    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Symbol);
    assert!(app.symbol_state().is_some());
    assert!(app.show_side_panel);
    assert_eq!(app.mode, Mode::SidePanel);

    app.handle_action(Action::ToggleSidePanel);
    assert!(!app.show_side_panel);
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Symbol);
    assert!(app.symbol_state().is_some());
    assert_eq!(app.mode, Mode::Normal);

    app.handle_action(Action::ToggleSidePanel);
    assert!(app.show_side_panel);
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Symbol);
    assert!(app.symbol_state().is_some());
    assert_eq!(app.mode, Mode::SidePanel);
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols"))]
#[test]
fn hex_edit_does_not_replace_symbol_panel_with_inspector() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0xc3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.execute_command(Command::Symbols).unwrap();

    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Symbol);
    assert!(app.symbol_state().is_some());

    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app.cursor = 0x100;
    app.edit_nibble(0xC).unwrap();

    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Symbol);
    assert!(app.symbol_state().is_some());
    assert!(app.show_side_panel);

    app.execute_command(Command::SymbolsOff).unwrap();
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Inspector);
    assert!(app.inspector().is_some() || app.inspector_error.is_some());
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_analysis_rejects_input_over_128_mib() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("large.bin");
    let handle = fs::File::create(&file).unwrap();
    handle.set_len(128 * 1024 * 1024 + 1).unwrap();
    drop(handle);
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
    };
    let mut app = App::from_cli(cli).unwrap();

    let err = app
        .execute_command(Command::Analysis(
            crate::commands::types::AnalysisCommand::Run,
        ))
        .unwrap_err();

    assert!(err.to_string().contains("128 MiB"));
    assert!(app.analysis_state.is_none());
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_analysis_repeated_run_while_running_is_ignored() {
    let bytes = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);

    app.execute_command(Command::Analysis(
        crate::commands::types::AnalysisCommand::Run,
    ))
    .unwrap();
    let job_id = app.analysis_job_id;
    assert!(matches!(
        app.analysis_state.as_ref().map(|state| &state.status),
        Some(super::super::analysis_state::SagittaStatus::Running)
    ));

    app.execute_command(Command::Analysis(
        crate::commands::types::AnalysisCommand::Run,
    ))
    .unwrap();

    assert_eq!(app.analysis_job_id, job_id);
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_analysis_rejects_unsupported_input_without_replacing_symbols() {
    let mut app = app_with_bytes(&[0, 1, 2, 3]);
    app.symbol_state = Some(native_test_symbol_state("native_entry"));

    let err = app
        .execute_command(Command::Analysis(
            crate::commands::types::AnalysisCommand::Run,
        ))
        .unwrap_err();

    assert!(err.to_string().contains("unsupported format"));
    assert!(app.analysis_state.is_none());
    let state = app.symbol_state().unwrap();
    assert_eq!(
        state.source,
        crate::app::symbol_state::SymbolPanelSource::Native
    );
    assert_eq!(state.entries[0].name, "native_entry");

    let mut aarch64_elf = build_disassembly_elf64(&[0x90, 0xc3]);
    aarch64_elf[18..20].copy_from_slice(&183u16.to_le_bytes());
    let mut app2 = app_with_bytes(&aarch64_elf);
    let err = app2
        .execute_command(Command::Analysis(
            crate::commands::types::AnalysisCommand::Run,
        ))
        .unwrap_err();

    assert!(err.to_string().contains("unsupported arch"));
    assert!(app2.analysis_state.is_none());
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_result_opens_symbol_panel_without_stealing_command_focus() {
    let mut app = app_with_bytes(&[0x7f, b'E', b'L', b'F']);
    app.mode = Mode::Command;
    app.analysis_job_id = 7;
    app.background_tx
        .send(crate::app::BackgroundJobResult::SagittaAnalysis {
            job_id: 7,
            revision: app.document_revision,
            result: Ok(sagitta_test_snapshot()),
        })
        .unwrap();

    app.drain_background_results();

    assert_eq!(app.mode, Mode::Command);
    assert!(app.show_side_panel);
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Symbol);
    let state = app.symbol_state().unwrap();
    assert_eq!(
        state.source,
        crate::app::symbol_state::SymbolPanelSource::Sagitta
    );
    assert_eq!(state.entries[0].name, "sub_401000");
    assert_eq!(
        state.entries[0].name_kind,
        crate::app::symbol_state::SymbolNameKind::Synthetic
    );
    assert_eq!(state.entries[0].logical_offset, Some(1));
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_failed_result_does_not_replace_symbol_panel() {
    let mut app = app_with_bytes(&[0, 1, 2, 3]);
    app.analysis_job_id = 9;
    app.symbol_state = Some(native_test_symbol_state("native_entry"));

    app.background_tx
        .send(crate::app::BackgroundJobResult::SagittaAnalysis {
            job_id: 9,
            revision: app.document_revision,
            result: Err("Sagitta panic: boom".to_owned()),
        })
        .unwrap();
    app.drain_background_results();

    assert!(matches!(
        app.analysis_state.as_ref().map(|state| &state.status),
        Some(super::super::analysis_state::SagittaStatus::Failed(message))
            if message.contains("Sagitta panic")
    ));
    let state = app.symbol_state().unwrap();
    assert_eq!(
        state.source,
        crate::app::symbol_state::SymbolPanelSource::Native
    );
    assert_eq!(state.entries[0].name, "native_entry");
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_symbols_command_prefers_ready_snapshot() {
    let mut app = app_with_bytes(&[0, 1, 2, 3]);
    app.symbol_state = Some(native_test_symbol_state("native_entry"));
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot()),
    });

    app.execute_command(Command::Symbols).unwrap();

    let state = app.symbol_state().unwrap();
    assert_eq!(
        state.source,
        crate::app::symbol_state::SymbolPanelSource::Sagitta
    );
    assert_eq!(state.entries[0].name, "sub_401000");
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_off_ignores_late_running_result() {
    let mut app = app_with_bytes(&[0, 1, 2, 3]);
    let cancelled_job = 7;
    app.analysis_job_id = cancelled_job;
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Running,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: None,
    });

    app.execute_command(Command::Analysis(
        crate::commands::types::AnalysisCommand::Off,
    ))
    .unwrap();
    assert!(app.analysis_state.is_none());
    assert_ne!(app.analysis_job_id, cancelled_job);

    app.background_tx
        .send(crate::app::BackgroundJobResult::SagittaAnalysis {
            job_id: cancelled_job,
            revision: app.document_revision,
            result: Ok(sagitta_test_snapshot()),
        })
        .unwrap();
    app.drain_background_results();

    assert!(app.analysis_state.is_none());
    assert!(app.symbol_state().is_none());
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_validity_tracks_replacement_vs_layout_edits() {
    let mut app = app_with_bytes(&[0, 1, 2, 3]);
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot_for(0, Some(0x100), "sub_0")),
    });

    app.apply_paste_overwrite(&[0xff]).unwrap();
    assert_eq!(
        app.analysis_state.as_ref().unwrap().validity,
        super::super::analysis_state::AnalysisValidity::OutdatedBytes
    );

    let mut bulk = app_with_bytes(&[0, 1, 2, 3]);
    bulk.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: bulk.document_revision,
        snapshot: Some(sagitta_test_snapshot_for(0, Some(0x100), "sub_0")),
    });
    bulk.execute_command(Command::Fill {
        pattern: vec![0xff],
        len: 2,
    })
    .unwrap();
    assert!(matches!(
        bulk.undo_stack.last().unwrap().ops[0],
        EditOp::ReplaceBulk { .. }
    ));
    assert_eq!(
        bulk.analysis_state.as_ref().unwrap().validity,
        super::super::analysis_state::AnalysisValidity::OutdatedBytes
    );

    app.apply_paste_insert(&[0xee]).unwrap();
    assert_eq!(
        app.analysis_state.as_ref().unwrap().validity,
        super::super::analysis_state::AnalysisValidity::InvalidLayout
    );
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_invalidates_tombstone_real_delete_and_resize_replace() {
    let mut tombstone = app_with_bytes(&[0, 1, 2, 3]);
    tombstone.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: tombstone.document_revision,
        snapshot: Some(sagitta_test_snapshot()),
    });
    tombstone.delete_current().unwrap();
    assert_eq!(
        tombstone.analysis_state.as_ref().unwrap().validity,
        super::super::analysis_state::AnalysisValidity::InvalidLayout
    );

    let mut real_delete = app_with_bytes(&[0, 1, 2, 3]);
    real_delete.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: real_delete.document_revision,
        snapshot: Some(sagitta_test_snapshot()),
    });
    real_delete.mode = Mode::InsertHex { pending: None };
    real_delete.cursor = 1;
    real_delete.edit_backspace().unwrap();
    assert_eq!(
        real_delete.analysis_state.as_ref().unwrap().validity,
        super::super::analysis_state::AnalysisValidity::InvalidLayout
    );

    let mut resize = app_with_bytes(&[0, 1, 2, 3]);
    resize.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: resize.document_revision,
        snapshot: Some(sagitta_test_snapshot()),
    });
    resize
        .execute_command(Command::Replace {
            needle: vec![1],
            replacement: vec![0xaa, 0xbb],
            allow_resize: true,
            force: false,
        })
        .unwrap();
    assert_eq!(
        resize.analysis_state.as_ref().unwrap().validity,
        super::super::analysis_state::AnalysisValidity::InvalidLayout
    );
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_symbol_jump_outdated_allowed_invalid_layout_rejected() {
    let mut app = app_with_bytes(&[0, 1, 2, 3]);
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot_for(0x401000, Some(1), "sub_401000")),
    });
    assert!(app.open_sagitta_symbol_panel_if_ready());
    app.apply_paste_overwrite(&[0xff]).unwrap();

    app.navigate_to_selected_symbol().unwrap();

    assert_eq!(app.cursor, 1);
    assert_eq!(app.status_level, StatusLevel::Warning);
    assert!(app.status_message.contains("analysis outdated; rerun :ana"));

    let mut invalid = app_with_bytes(&[0, 1, 2, 3]);
    invalid.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: invalid.document_revision,
        snapshot: Some(sagitta_test_snapshot_for(0x401000, Some(1), "sub_401000")),
    });
    assert!(invalid.open_sagitta_symbol_panel_if_ready());
    invalid.apply_paste_insert(&[0xee]).unwrap();

    let err = invalid.navigate_to_selected_symbol().unwrap_err();

    assert!(err.to_string().contains("analysis offsets changed"));
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
#[test]
fn sagitta_snapshot_annotates_disassembly_rows_and_symbol_search() {
    let bytes =
        build_disassembly_elf64_with_symbol(&[0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xC3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot()),
    });

    let state = app.current_disassembly_state().unwrap();
    let rows = app.collect_disassembly_rows(&state, 0x100, 3).unwrap();

    assert_eq!(rows[0].symbol_label.as_deref(), Some("sub_401000"));
    assert_eq!(
        rows[1]
            .direct_target
            .as_ref()
            .and_then(|target| target.display_name.as_deref()),
        Some("sub_401000")
    );

    app.execute_command(Command::SearchSymbol {
        pattern: "sub_401000".to_owned(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 0x101);
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
#[test]
fn sagitta_result_adds_function_rail_to_cached_disassembly_rows() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0x90, 0xC3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let state = app.current_disassembly_state().unwrap();
    let rows_before = app.collect_disassembly_rows(&state, 0x100, 3).unwrap();
    assert_eq!(rows_before.len(), 3);
    assert!(rows_before.iter().all(|row| row.function_scope.is_none()));

    app.analysis_job_id = 11;
    app.background_tx
        .send(crate::app::BackgroundJobResult::SagittaAnalysis {
            job_id: 11,
            revision: app.document_revision,
            result: Ok(sagitta_test_snapshot_with_block(
                0x401000,
                Some(0x100),
                "sub_401000",
                0x401003,
            )),
        })
        .unwrap();
    app.drain_background_results();

    let rows_after = app.collect_disassembly_rows(&state, 0x100, 3).unwrap();

    assert_eq!(
        rows_after[0]
            .function_scope
            .as_ref()
            .map(|scope| scope.boundary),
        Some(crate::disasm::DisasmFunctionBoundary::Entry)
    );
    assert_eq!(
        rows_after[1]
            .function_scope
            .as_ref()
            .map(|scope| scope.boundary),
        Some(crate::disasm::DisasmFunctionBoundary::Body)
    );
    assert_eq!(
        rows_after[2]
            .function_scope
            .as_ref()
            .map(|scope| scope.boundary),
        Some(crate::disasm::DisasmFunctionBoundary::Exit)
    );
    assert_eq!(
        rows_after[0]
            .function_scope
            .as_ref()
            .map(|scope| scope.name.as_str()),
        Some("sub_401000")
    );
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
#[test]
fn sagitta_large_snapshot_annotations_do_not_scan_all_functions_per_row() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0x90, 0xC3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: Some(sagitta_large_test_snapshot_with_target(8192)),
    });

    let state = app.current_disassembly_state().unwrap();
    for _ in 0..8 {
        let rows = app.collect_disassembly_rows(&state, 0x100, 3).unwrap();
        assert_eq!(
            rows[0].function_scope.as_ref().map(|scope| scope.boundary),
            Some(crate::disasm::DisasmFunctionBoundary::Entry)
        );
        assert_eq!(
            rows[1].function_scope.as_ref().map(|scope| scope.boundary),
            Some(crate::disasm::DisasmFunctionBoundary::Body)
        );
        assert_eq!(
            rows[2].function_scope.as_ref().map(|scope| scope.boundary),
            Some(crate::disasm::DisasmFunctionBoundary::Exit)
        );
        assert_eq!(rows[0].symbol_label.as_deref(), Some("sub_401000"));
    }
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
#[test]
fn sagitta_function_rail_spans_alignment_gap_between_blocks() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0x90, 0x90, 0xC3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot_with_blocks(
            0x401000,
            Some(0x100),
            "sub_401000",
            vec![
                (0x401000, 0x401001, Some(0x100)),
                (0x401003, 0x401004, Some(0x103)),
            ],
        )),
    });

    let state = app.current_disassembly_state().unwrap();
    let rows = app.collect_disassembly_rows(&state, 0x100, 4).unwrap();

    assert_eq!(
        rows[0].function_scope.as_ref().map(|scope| scope.boundary),
        Some(crate::disasm::DisasmFunctionBoundary::Entry)
    );
    assert_eq!(
        rows[1].function_scope.as_ref().map(|scope| scope.boundary),
        Some(crate::disasm::DisasmFunctionBoundary::Body)
    );
    assert_eq!(
        rows[2].function_scope.as_ref().map(|scope| scope.boundary),
        Some(crate::disasm::DisasmFunctionBoundary::Body)
    );
    assert_eq!(
        rows[3].function_scope.as_ref().map(|scope| scope.boundary),
        Some(crate::disasm::DisasmFunctionBoundary::Exit)
    );
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
#[test]
fn sagitta_outdated_bytes_keep_function_rail_marked_stale() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0x90, 0xC3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::OutdatedBytes,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot_with_block(
            0x401000,
            Some(0x100),
            "sub_401000",
            0x401003,
        )),
    });

    let state = app.current_disassembly_state().unwrap();
    let rows = app.collect_disassembly_rows(&state, 0x100, 3).unwrap();

    assert!(rows
        .iter()
        .all(|row| row.function_scope.as_ref().is_some_and(|scope| scope.stale)));
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
#[test]
fn sagitta_invalid_layout_disables_disassembly_annotations() {
    let bytes =
        build_disassembly_elf64_with_symbol(&[0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xC3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.analysis_state = Some(super::super::analysis_state::SagittaAnalysisState {
        status: super::super::analysis_state::SagittaStatus::Ready,
        validity: super::super::analysis_state::AnalysisValidity::InvalidLayout,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot_with_block(
            0x401000,
            Some(0x100),
            "sub_401000",
            0x401007,
        )),
    });

    let state = app.current_disassembly_state().unwrap();
    let rows = app.collect_disassembly_rows(&state, 0x100, 3).unwrap();

    assert_eq!(rows[0].symbol_label.as_deref(), Some("entry"));
    assert_eq!(
        rows[1]
            .direct_target
            .as_ref()
            .and_then(|target| target.display_name.as_deref()),
        Some("entry")
    );
    assert!(rows.iter().all(|row| row.function_scope.is_none()));
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols"))]
#[test]
fn symbol_panel_scrolls_and_mouse_click_navigates() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0x90, 0x90, 0x90, 0xc3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.execute_command(Command::Symbols).unwrap();

    if let Some(state) = app.symbol_state_mut() {
        for index in 1..4 {
            state
                .entries
                .push(crate::app::symbol_state::SymbolPanelEntry {
                    address: 0x401000 + index,
                    name: format!("target_{index}"),
                    name_kind: crate::app::symbol_state::SymbolNameKind::Real,
                    size: 0,
                    symbol_type: crate::executable::SymbolType::Function,
                    source: crate::app::symbol_state::SymbolPanelEntrySource::Dynamic,
                    logical_offset: None,
                    file_offset: Some(0x100 + index),
                    confidence_label: None,
                });
        }
        state
            .entries
            .push(crate::app::symbol_state::SymbolPanelEntry {
                address: 0x401004,
                name: "very_long_symbol_name_that_wraps_across_multiple_detail_rows".to_owned(),
                name_kind: crate::app::symbol_state::SymbolNameKind::Real,
                size: 0,
                symbol_type: crate::executable::SymbolType::Function,
                source: crate::app::symbol_state::SymbolPanelEntrySource::Dynamic,
                logical_offset: None,
                file_offset: Some(0x104),
                confidence_label: None,
            });
    }

    app.view_rows = 8;
    app.scroll_symbol_panel(2);
    assert_eq!(app.symbol_state().unwrap().scroll_offset, 2);

    app.last_columns = Some(crate::view::layout::MainColumns {
        main_pane_kind: crate::view::layout::MainPaneKind::Disassembly,
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
        row: 2,
        modifiers: crossterm::event::KeyModifiers::empty(),
    });

    assert_eq!(app.symbol_state().unwrap().selected_row, 3);
    assert_eq!(app.cursor, 0x103);

    app.set_symbol_selected_row(4);
    app.scroll_symbol_detail(3, 24);
    assert!(app.symbol_state().unwrap().detail_scroll_offset > 0);
}

#[cfg(feature = "disasm-capstone")]
#[test]
fn disassemble_force_and_off_commands() {
    // Force command with explicit arch
    let mut bytes = vec![0_u8; 0x40];
    bytes[0x10..0x12].copy_from_slice(&[0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::DisassembleForce {
        arch: "x86_64".to_owned(),
        offset: 0x10,
    })
    .unwrap();
    assert_eq!(app.cursor, 0x10);
    assert!(app.status_message.contains("Raw x86_64"));

    // Off command returns to hex view
    let bytes2 = build_disassembly_elf64(&[0x90, 0x90, 0x90, 0xc3]);
    let mut app2 = app_with_bytes(&bytes2);
    app2.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app2.execute_command(Command::DisassembleOff).unwrap();
    assert!(matches!(app2.main_view, crate::app::MainView::Hex));
    assert!(app2.disasm_backend.is_none());
}

#[cfg(feature = "disasm-capstone")]
#[test]
fn disassembly_navigation_and_scroll() {
    let bytes = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    // Vertical move uses instruction boundaries
    assert_eq!(app.cursor, 0x100);
    app.move_vertical(1);
    assert_eq!(app.cursor, 0x101);
    app.move_vertical(2);
    assert_eq!(app.cursor, 0x105);
    app.move_vertical(-1);
    assert_eq!(app.cursor, 0x104);

    // Scroll viewport uses instruction rows
    let mut app2 = app_with_bytes(&bytes);
    app2.view_rows = 2;
    app2.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app2.scroll_viewport(1);
    match &app2.main_view {
        crate::app::MainView::Disassembly(state) => assert_eq!(state.viewport_top, 0x101),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }

    // Scroll up from raw tail does not snap back to text end
    let bytes3 = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app3 = app_with_bytes(&bytes3);
    app3.view_rows = 2;
    app3.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app3.scroll_viewport(99);
    let bottom_top = match &app3.main_view {
        crate::app::MainView::Disassembly(state) => state.viewport_top,
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    assert!(bottom_top >= 0x1f0);
    app3.scroll_viewport(-1);
    match &app3.main_view {
        crate::app::MainView::Disassembly(state) => {
            assert_eq!(state.viewport_top, bottom_top.saturating_sub(8));
            assert!(state.viewport_top > 0x102);
        }
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
}

#[cfg(feature = "disasm-capstone")]
#[test]
fn disassembly_search_variants() {
    let bytes = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.view_rows = 4;
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    // Instruction search jumps to matching row
    app.execute_command(Command::SearchInstruction {
        pattern: "ret".to_owned(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 0x105);

    // Byte search recenters to containing instruction row
    let mut app2 = app_with_bytes(&bytes);
    app2.view_rows = 4;
    app2.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app2.execute_command(Command::SearchHex {
        pattern: vec![0x89, 0xe5],
        backward: false,
        deprecated_alias: false,
    })
    .unwrap();
    assert_eq!(app2.cursor, 0x102);
}

#[cfg(feature = "disasm-capstone")]
#[test]
fn disassembly_instruction_search_does_not_pollute_view_cache() {
    let code = vec![0x90; 4096];
    let bytes = build_disassembly_elf64(&code);
    let mut app = app_with_bytes(&bytes);
    app.config.search_wrap = false;
    app.view_rows = 8;
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    let state = match &app.main_view {
        crate::app::MainView::Disassembly(state) => state.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let visible_rows = app
        .collect_disassembly_rows(&state, state.viewport_top, 8)
        .unwrap();
    assert!(!visible_rows.is_empty());
    let before = app
        .disasm_cache
        .as_ref()
        .map(|cache| cache.cached_row_count())
        .unwrap_or_default();

    app.execute_command(Command::SearchInstruction {
        pattern: "ret".to_owned(),
        backward: false,
    })
    .unwrap();

    let after = app
        .disasm_cache
        .as_ref()
        .map(|cache| cache.cached_row_count())
        .unwrap_or_default();
    assert_eq!(after, before);
    assert!(app.status_message.contains("not found"));
    assert!(app.status_message.contains("wrap off"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Disassembly view editing: nibble edit, undo, redo, fill, replace
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "disasm-capstone")]
#[test]
fn disassembly_editing_undo_redo_and_fill() {
    // Nibble edit updates instruction text
    let bytes = build_disassembly_elf64(&[0x90, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let initial_bytes = app.document.read_logical_range(0x100, 3).unwrap();
    assert_eq!(initial_bytes, vec![0x90, 0x90, 0xc3]);

    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app.edit_nibble(0xC).unwrap();
    app.edit_nibble(0xC).unwrap();

    let after_bytes = app.document.read_logical_range(0x100, 3).unwrap();
    assert_eq!(after_bytes, vec![0xCC, 0x90, 0xc3]);
    let state = match &app.main_view {
        crate::app::MainView::Disassembly(s) => s.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows = app
        .collect_disassembly_rows(&state, state.viewport_top, 3)
        .unwrap();
    assert!(
        !rows[0].text.contains("nop"),
        "should be int3, got: {}",
        rows[0].text
    );
    assert!(rows[0].text.contains("int3"));

    // Undo restores original instruction
    let bytes2 = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app2 = app_with_bytes(&bytes2);
    app2.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app2.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    app2.edit_nibble(0xC).unwrap();
    app2.edit_nibble(0xC).unwrap();
    app2.undo(2, false).unwrap();
    app2.mode = Mode::Normal;
    let restored = app2.document.read_logical_range(0x100, 2).unwrap();
    assert_eq!(restored, vec![0x90, 0xc3]);

    // Redo reapplies change
    app2.redo(2, false).unwrap();
    let state2 = match &app2.main_view {
        crate::app::MainView::Disassembly(s) => s.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows2 = app2
        .collect_disassembly_rows(&state2, state2.viewport_top, 2)
        .unwrap();
    assert!(rows2[0].text.contains("int3"));

    // Fill command updates instructions
    let bytes3 = build_disassembly_elf64(&[0x90, 0x90, 0xc3]);
    let mut app3 = app_with_bytes(&bytes3);
    app3.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app3.execute_command(Command::Fill {
        pattern: vec![0xcc],
        len: 2,
    })
    .unwrap();
    let state3 = match &app3.main_view {
        crate::app::MainView::Disassembly(s) => s.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows3 = app3
        .collect_disassembly_rows(&state3, state3.viewport_top, 3)
        .unwrap();
    assert!(rows3[0].text.contains("int3"));
    assert!(rows3[1].text.contains("int3"));
}

#[cfg(all(feature = "disasm-capstone", not(feature = "asm")))]
#[test]
fn disassembly_inline_assemble_requires_asm_feature() {
    let bytes = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.handle_action(Action::BeginDisasmEdit);

    assert_eq!(app.mode, Mode::Normal);
    assert!(app.disasm_edit.is_none());
    assert_eq!(app.status_level, StatusLevel::Error);
    assert!(app
        .status_message
        .contains("keystone backend is not enabled"));
}

#[cfg(all(feature = "disasm-capstone", feature = "asm"))]
#[test]
fn disassembly_inline_assemble_invalid_submit_exits_with_error() {
    let bytes = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.handle_action(Action::BeginDisasmEdit);
    assert_eq!(app.mode, Mode::DisasmEdit);
    app.disasm_edit.as_mut().unwrap().buffer = "definitely_not_valid".to_owned();

    app.handle_action(Action::CommandSubmit);

    assert_eq!(app.mode, Mode::Normal);
    assert!(app.disasm_edit.is_none());
    assert_eq!(app.status_level, StatusLevel::Error);
    assert!(app.status_message.contains("assembly error"));
}

#[cfg(all(feature = "disasm-capstone", feature = "asm"))]
#[test]
fn disassembly_inline_assemble_shorter_patch_nop_fills_current_instruction() {
    let bytes = build_disassembly_elf64(&[0x48, 0x83, 0xec, 0x20, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.handle_action(Action::BeginDisasmEdit);
    app.disasm_edit.as_mut().unwrap().buffer = "nop".to_owned();
    app.handle_action(Action::CommandSubmit);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.document.read_logical_range(0x100, 5).unwrap(),
        vec![0x90, 0x90, 0x90, 0x90, 0xc3]
    );
    assert_eq!(app.status_level, StatusLevel::Info);
    assert!(app.status_message.contains("3 trailing nop"));
}

#[cfg(all(feature = "disasm-capstone", feature = "asm"))]
#[test]
fn disassembly_inline_assemble_longer_patch_warns_and_nops_truncated_tail() {
    let bytes = build_disassembly_elf64(&[0x83, 0xc0, 0x01, 0x55, 0x48, 0x89, 0xe5, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.handle_action(Action::BeginDisasmEdit);
    app.disasm_edit.as_mut().unwrap().buffer = "mov eax, 0x11223344".to_owned();
    app.handle_action(Action::CommandSubmit);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.document.read_logical_range(0x100, 8).unwrap(),
        vec![0xb8, 0x44, 0x33, 0x22, 0x11, 0x90, 0x90, 0xc3]
    );
    assert_eq!(app.status_level, StatusLevel::Warning);
    assert!(app.status_message.contains("covered 3 rows"));
    assert!(app.status_message.contains("trailing nop 2"));
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols", feature = "asm"))]
#[test]
fn disassembly_inline_assemble_uses_raw_text_without_breaking_symbolized_display() {
    let bytes =
        build_disassembly_elf64_with_symbol(&[0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xC3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.cursor = 0x101;

    let state = match &app.main_view {
        crate::app::MainView::Disassembly(state) => state.clone(),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    let rows = app
        .collect_disassembly_rows(&state, state.viewport_top, 3)
        .unwrap();
    assert_eq!(rows[1].text, "call entry");
    assert_ne!(rows[1].assembly_text, rows[1].text);
    assert!(rows[1].assembly_text.contains("0x401000"));

    app.handle_action(Action::BeginDisasmEdit);

    assert_eq!(app.mode, Mode::DisasmEdit);
    assert_eq!(
        app.disasm_edit.as_ref().unwrap().buffer,
        rows[1].assembly_text
    );
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols", feature = "asm"))]
#[test]
fn disassembly_inline_assemble_resolves_direct_symbol_name() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0x90, 0x90, 0x90, 0x90, 0xc3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.cursor = 0x101;

    app.handle_action(Action::BeginDisasmEdit);
    app.disasm_edit.as_mut().unwrap().buffer = "call entry".to_owned();
    app.handle_action(Action::CommandSubmit);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.document.read_logical_range(0x101, 5).unwrap(),
        vec![0xe8, 0xfa, 0xff, 0xff, 0xff]
    );
    assert_eq!(app.status_level, StatusLevel::Warning);
    assert!(app.status_message.contains("resolved entry -> 0x401000"));
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols", feature = "asm"))]
#[test]
fn disassembly_inline_assemble_resolves_import_target_name() {
    let bytes = build_disassembly_elf64_with_symbol(&[0x90, 0x90, 0x90, 0x90, 0x90, 0xc3], "entry");
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    if let crate::app::MainView::Disassembly(state) = &mut app.main_view {
        state
            .info
            .target_names_by_va
            .insert(0x401030, "strcmp".to_owned());
        state
            .info
            .target_names_by_name
            .insert("strcmp".to_owned(), vec![0x401030]);
    }
    app.cursor = 0x100;

    app.handle_action(Action::BeginDisasmEdit);
    app.disasm_edit.as_mut().unwrap().buffer = "call strcmp".to_owned();
    app.handle_action(Action::CommandSubmit);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.document.read_logical_range(0x100, 5).unwrap(),
        vec![0xe8, 0x2b, 0x00, 0x00, 0x00]
    );
    assert!(app.status_message.contains("resolved strcmp -> 0x401030"));
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols", feature = "asm"))]
#[test]
fn disassembly_inline_assemble_unknown_symbol_keeps_document_unchanged() {
    let bytes = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let before = app.document.read_logical_range(0x100, 2).unwrap();

    app.handle_action(Action::BeginDisasmEdit);
    app.disasm_edit.as_mut().unwrap().buffer = "call missing_symbol".to_owned();
    app.handle_action(Action::CommandSubmit);

    assert_eq!(app.mode, Mode::Normal);
    assert!(app.disasm_edit.is_none());
    assert_eq!(app.status_level, StatusLevel::Error);
    assert!(app.status_message.contains("unknown patch symbol"));
    assert_eq!(app.document.read_logical_range(0x100, 2).unwrap(), before);
}

#[test]
fn fixed_size_document_blocks_insert_mode_and_eof_cursor() {
    let mut app = app_with_fixed_size_bytes(&[0x12, 0x34]);

    app.handle_action(Action::EnterInsert);
    assert!(app.status_message.contains("fixed-size"));
    assert!(matches!(app.mode, Mode::Normal));

    app.mode = Mode::EditHex {
        phase: NibblePhase::High,
    };
    assert_eq!(app.clamp_cursor_for_mode(app.document.len(), app.mode), 1);

    app.cursor = 1;
    app.handle_action(Action::EditHex(0xf));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xf4));
    assert_eq!(app.document.len(), 2);
    assert_eq!(app.document.visible_len(), 2);
}
