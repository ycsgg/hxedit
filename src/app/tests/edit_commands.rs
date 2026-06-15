use super::*;

// ═══════════════════════════════════════════════════════════════════════════
// Paste: overwrite and insert with undo/redo
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn paste_overwrite_and_insert_with_undo_redo() {
    // Overwrite replaces in place
    let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
    app.cursor = 1;
    assert_eq!(app.apply_paste_overwrite(&[0xaa, 0xbb]).unwrap(), 2);
    assert_eq!(app.document.len(), 3);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));

    // Undo reverts overwrite paste
    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x22));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x33));

    // Redo reapplies overwrite paste
    app.redo(1, true).unwrap();
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));

    // Undo reverts insert paste
    let mut app2 = app_with_bytes(&[0x11, 0x22]);
    app2.cursor = 1;
    app2.apply_paste_insert(&[0xaa, 0xbb]).unwrap();
    app2.undo(1, true).unwrap();
    app2.redo(1, true).unwrap();
    assert_eq!(app2.document.len(), 4);
    assert_eq!(app2.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
}

#[test]
fn paste_overwrite_clean_range_uses_bulk_bytes_undo() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13, 0x14]);
    app.cursor = 1;

    assert_eq!(app.apply_paste_overwrite(&[0xaa, 0x12, 0xbb]).unwrap(), 3);
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0xaa, 0x12, 0xbb, 0x14]
    );
    assert_eq!(app.document.replacement_dirty_bytes(), 2);

    let step = app.undo_stack.last().expect("paste should push undo");
    assert_eq!(step.ops.len(), 2);
    match &step.ops[0] {
        EditOp::ReplaceBulk {
            offset,
            len,
            before: BulkReplacement::Clear,
            after: BulkReplacement::Bytes(bytes),
        } => {
            assert_eq!((*offset, *len), (1, 1));
            assert_eq!(bytes.as_ref(), &[0xaa]);
        }
        other => panic!("unexpected first paste op: {other:?}"),
    }
    match &step.ops[1] {
        EditOp::ReplaceBulk {
            offset,
            len,
            before: BulkReplacement::Clear,
            after: BulkReplacement::Bytes(bytes),
        } => {
            assert_eq!((*offset, *len), (3, 1));
            assert_eq!(bytes.as_ref(), &[0xbb]);
        }
        other => panic!("unexpected second paste op: {other:?}"),
    }

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0x11, 0x12, 0x13, 0x14]
    );
    assert_eq!(app.document.replacement_dirty_bytes(), 0);

    app.redo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0xaa, 0x12, 0xbb, 0x14]
    );
    assert_eq!(app.document.replacement_dirty_bytes(), 2);
}

#[test]
fn paste_overwrite_existing_replacement_uses_mixed_patch_undo() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.document.replace_display_byte(1, 0xab).unwrap();
    app.cursor = 0;

    assert_eq!(app.apply_paste_overwrite(&[0xff, 0xee, 0xdd]).unwrap(), 3);

    let step = app.undo_stack.last().expect("paste should push undo");
    assert!(matches!(
        &step.ops[0],
        EditOp::ReplacePatch {
            offset: 0,
            len: 3,
            ..
        }
    ));
    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
    app.redo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xff, 0xee, 0xdd, 0x13]
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Fill, Export, Replace commands
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fill_command_repeats_pattern_with_undo() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13, 0x14]);
    app.cursor = 1;
    app.execute_command(Command::Fill {
        pattern: vec![0xaa, 0xbb],
        len: 3,
    })
    .unwrap();

    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0xaa));
    assert!(app.status_message.contains("filled 3 bytes"));

    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x11));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
}

#[test]
fn fill_clean_range_uses_bulk_undo() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13, 0x14]);
    app.cursor = 1;

    app.execute_command(Command::Fill {
        pattern: vec![0xaa, 0xbb],
        len: 3,
    })
    .unwrap();

    let step = app.undo_stack.last().expect("fill should push undo");
    assert_eq!(step.ops.len(), 1);
    assert!(matches!(
        &step.ops[0],
        EditOp::ReplaceBulk {
            offset: 1,
            len: 3,
            before: BulkReplacement::Clear,
            after: BulkReplacement::Pattern(pattern),
        } if pattern == &vec![0xaa, 0xbb]
    ));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0x11, 0x12, 0x13, 0x14]
    );
    app.redo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0xaa, 0xbb, 0xaa, 0x14]
    );
}

#[test]
fn fill_matching_pattern_still_pushes_undo_and_marks_dirty() {
    let mut app = app_with_bytes(&[0xaa, 0xbb, 0xaa, 0xbb]);

    app.execute_command(Command::Fill {
        pattern: vec![0xaa, 0xbb],
        len: 4,
    })
    .unwrap();

    assert_eq!(app.undo_stack.len(), 1);
    assert!(app.document.is_dirty());
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xaa, 0xbb, 0xaa, 0xbb]
    );

    app.undo(1, true).unwrap();
    assert!(!app.document.is_dirty());
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xaa, 0xbb, 0xaa, 0xbb]
    );
}

#[test]
fn fill_existing_replacement_uses_mixed_patch_undo() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.cursor = 1;
    app.document.replace_display_byte(1, 0xab).unwrap();
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));

    app.cursor = 0;
    app.execute_command(Command::Fill {
        pattern: vec![0xff],
        len: 3,
    })
    .unwrap();

    let step = app.undo_stack.last().expect("fill should push undo");
    assert!(matches!(
        &step.ops[0],
        EditOp::ReplacePatch {
            offset: 0,
            len: 3,
            ..
        }
    ));
    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
    app.redo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xff, 0xff, 0xff, 0x13]
    );
}

#[test]
fn export_command_writes_logical_selection() {
    // From visual selection
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

    // From inspector field
    let mut app2 = app_with_inspector_field(b"hello world", 6, 5);
    let path2 = dir.path().join("field.bin");
    app2.execute_command(Command::Export {
        format: ExportFormat::Binary {
            path: path2.clone(),
        },
    })
    .unwrap();

    assert_eq!(fs::read(&path2).unwrap(), b"world");
}

#[test]
fn replace_command_variants() {
    // Equal length replace
    let mut app = app_with_bytes(b"abcabc");
    app.execute_command(Command::Replace {
        needle: b"ab".to_vec(),
        replacement: b"xy".to_vec(),
        allow_resize: false,
        force: false,
    })
    .unwrap();
    assert_eq!(app.document.len(), 6);
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"xycxyc");
    assert!(app.status_message.contains("replaced 2 matches"));
    app.undo(1, true).unwrap();
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"abcabc");

    // Resize replace
    let mut app2 = app_with_bytes(b"abcabc");
    app2.execute_command(Command::Replace {
        needle: b"ab".to_vec(),
        replacement: b"Z".to_vec(),
        allow_resize: true,
        force: false,
    })
    .unwrap();
    assert_eq!(app2.document.len(), 4);
    assert_eq!(app2.document.logical_bytes(0, 3).unwrap(), b"ZcZc");
    assert!(app2.status_message.contains("4→2 bytes"));

    // Visual selection scope
    let mut app3 = app_with_bytes(b"abxxab");
    app3.toggle_visual();
    app3.move_horizontal(3);
    app3.execute_command(Command::Replace {
        needle: b"ab".to_vec(),
        replacement: b"xy".to_vec(),
        allow_resize: false,
        force: false,
    })
    .unwrap();
    assert_eq!(app3.document.logical_bytes(0, 5).unwrap(), b"xyxxab");
    assert_eq!(app3.mode, Mode::Normal);
}

#[test]
fn replace_same_size_dirty_range_uses_mixed_patch_undo() {
    let mut app = app_with_bytes(b"abcabc");
    app.document
        .overwrite_run_pattern_overlay(0, 2, b"ab")
        .unwrap();
    assert!(app.document.is_dirty());
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"abcabc");

    app.execute_command(Command::Replace {
        needle: b"ab".to_vec(),
        replacement: b"xy".to_vec(),
        allow_resize: false,
        force: false,
    })
    .unwrap();

    let step = app.undo_stack.last().expect("replace should push undo");
    assert!(step.ops.iter().any(|op| matches!(
        op,
        EditOp::ReplacePatch {
            offset: 0,
            len: 2,
            ..
        }
    )));
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"xycxyc");

    app.undo(1, true).unwrap();
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"abcabc");
    assert_eq!(app.document.replacement_dirty_bytes(), 2);

    app.redo(1, true).unwrap();
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"xycxyc");
}

#[test]
fn replace_same_size_over_match_limit_requires_force() {
    let bytes = vec![0_u8; 65_536];
    let mut app = app_with_bytes(&bytes);

    app.execute_command(Command::Replace {
        needle: vec![0],
        replacement: vec![1],
        allow_resize: false,
        force: false,
    })
    .unwrap();

    assert!(app.undo_stack.is_empty());
    assert!(app.status_message.contains("more than 65535 matches"));
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0));
    assert_eq!(app.document.byte_at(65_535).unwrap(), ByteSlot::Present(0));

    let mut forced = app_with_bytes(&bytes);
    forced
        .execute_command(Command::Replace {
            needle: vec![0],
            replacement: vec![1],
            allow_resize: false,
            force: true,
        })
        .unwrap();

    assert_eq!(forced.document.byte_at(0).unwrap(), ByteSlot::Present(1));
    assert_eq!(
        forced.document.byte_at(65_535).unwrap(),
        ByteSlot::Present(1)
    );
    assert!(forced.status_message.contains("replaced 65536 matches"));
    forced.undo(1, true).unwrap();
    assert_eq!(forced.document.byte_at(0).unwrap(), ByteSlot::Present(0));
    assert_eq!(
        forced.document.byte_at(65_535).unwrap(),
        ByteSlot::Present(0)
    );
}

#[test]
fn replace_same_size_uses_non_overlapping_matches() {
    let mut app = app_with_bytes(b"aaaaa");

    app.execute_command(Command::Replace {
        needle: b"aa".to_vec(),
        replacement: b"bb".to_vec(),
        allow_resize: false,
        force: false,
    })
    .unwrap();

    assert_eq!(app.document.logical_bytes(0, 4).unwrap(), b"bbbba");
    assert!(app.status_message.contains("replaced 2 matches"));
}

#[test]
fn replace_same_size_finds_match_across_scan_chunk_boundary() {
    let mut bytes = vec![0x11_u8; 64 * 1024 + 4];
    bytes[64 * 1024 - 1] = 0xaa;
    bytes[64 * 1024] = 0xbb;
    let mut app = app_with_bytes(&bytes);

    app.execute_command(Command::Replace {
        needle: vec![0xaa, 0xbb],
        replacement: vec![0xcc, 0xdd],
        allow_resize: false,
        force: false,
    })
    .unwrap();

    assert_eq!(
        app.document.byte_at(64 * 1024 - 1).unwrap(),
        ByteSlot::Present(0xcc)
    );
    assert_eq!(
        app.document.byte_at(64 * 1024).unwrap(),
        ByteSlot::Present(0xdd)
    );
    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.byte_at(64 * 1024 - 1).unwrap(),
        ByteSlot::Present(0xaa)
    );
    assert_eq!(
        app.document.byte_at(64 * 1024).unwrap(),
        ByteSlot::Present(0xbb)
    );
}

#[test]
fn replace_same_size_does_not_match_across_tombstone() {
    let mut app = app_with_bytes(&[0xaa, 0xbb, 0xcc]);
    app.cursor = 1;
    app.delete_current().unwrap();

    app.execute_command(Command::Replace {
        needle: vec![0xaa, 0xcc],
        replacement: vec![0x11, 0x22],
        allow_resize: false,
        force: false,
    })
    .unwrap();

    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xcc));
    assert!(app.status_message.contains("no matches"));
}

#[test]
fn xor_command_copies_xored_logical_selection() {
    let mut app = app_with_bytes(&[0x0f, 0xf0, 0xaa, 0x55]);
    app.cursor = 1;
    app.delete_current().unwrap();
    app.cursor = 0;
    app.toggle_visual();
    app.move_horizontal(2);

    app.execute_command(Command::Xor {
        key: 0xff,
        in_place: false,
    })
    .unwrap();

    assert_eq!(test_clipboard_text(), "f0 55");
    assert_eq!(app.document.logical_bytes(0, 2).unwrap(), vec![0x0f, 0xaa]);
    assert!(app.status_message.contains("copied 2 logical bytes"));
    assert!(app.status_message.contains("display span 3"));
}

#[test]
fn xor_bang_replaces_selection_in_place_with_undo() {
    let mut app = app_with_bytes(&[0x0f, 0xf0, 0xaa, 0x55]);
    app.toggle_visual();
    app.move_horizontal(2);

    app.execute_command(Command::Xor {
        key: 0xff,
        in_place: true,
    })
    .unwrap();

    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xf0, 0x0f, 0x55, 0x55]
    );
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_range(), None);
    assert!(app.status_message.contains("replaced 3 logical bytes"));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0x0f, 0xf0, 0xaa, 0x55]
    );
}

#[test]
fn xor_bang_clean_range_uses_bulk_undo() {
    let mut app = app_with_bytes(&[0x0f, 0xf0, 0xaa, 0x55]);
    app.toggle_visual();
    app.move_horizontal(2);

    app.execute_command(Command::Xor {
        key: 0xff,
        in_place: true,
    })
    .unwrap();

    let step = app.undo_stack.last().expect("xor! should push undo");
    assert_eq!(step.ops.len(), 1);
    assert!(matches!(
        step.ops[0],
        EditOp::ReplaceBulk {
            offset: 0,
            len: 3,
            before: BulkReplacement::Clear,
            after: BulkReplacement::Xor { key: 0xff },
        }
    ));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0x0f, 0xf0, 0xaa, 0x55]
    );
    app.redo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xf0, 0x0f, 0x55, 0x55]
    );
}

#[test]
fn xor_bang_dirty_range_uses_mixed_patch_undo() {
    let mut app = app_with_bytes(&[0x0f, 0xf0, 0xaa, 0x55]);
    app.document
        .overwrite_run_pattern_overlay(0, 3, &[0x0f, 0xf0, 0xaa])
        .unwrap();
    assert!(app.document.is_dirty());
    app.toggle_visual();
    app.move_horizontal(2);

    app.execute_command(Command::Xor {
        key: 0xff,
        in_place: true,
    })
    .unwrap();

    let step = app.undo_stack.last().expect("xor! should push undo");
    assert!(matches!(
        &step.ops[0],
        EditOp::ReplacePatch {
            offset: 0,
            len: 3,
            ..
        }
    ));
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xf0, 0x0f, 0x55, 0x55]
    );

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0x0f, 0xf0, 0xaa, 0x55]
    );
    assert_eq!(app.document.replacement_dirty_bytes(), 3);

    app.redo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0xf0, 0x0f, 0x55, 0x55]
    );
}

#[test]
fn xor_bang_uses_inspector_field_selection() {
    let mut app = app_with_inspector_field(b"hello world", 6, 5);

    app.execute_command(Command::Xor {
        key: 0x20,
        in_place: true,
    })
    .unwrap();

    assert_eq!(app.document.logical_bytes(6, 10).unwrap(), b"WORLD");
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.status_message.contains("replaced 5 logical bytes"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Streaming transforms across the 64 KB chunk boundary
//
// :export / :fill / :xor! now walk pieces in 64 KB chunks instead of
// materializing the whole selection. These exercise ranges larger than one
// chunk to make sure chunk seams stay byte-accurate.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn export_streams_large_selection_across_chunk_boundary() {
    let size = 200_000usize; // > 3 * 64 KiB chunks
    let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    let mut app = app_with_bytes(&data);

    // Whole-file visual selection.
    app.cursor = 0;
    app.toggle_visual();
    app.move_horizontal((app.document.len() - 1) as i64);

    let dir = tempdir().unwrap();
    let path = dir.path().join("stream_export.bin");
    app.execute_command(Command::Export {
        format: ExportFormat::Binary { path: path.clone() },
    })
    .unwrap();

    assert_eq!(fs::read(&path).unwrap(), data);
    assert!(app
        .status_message
        .contains(&format!("exported {size} bytes")));
}

#[test]
fn export_streams_logical_bytes_skipping_tombstone() {
    let size = 130_000usize; // spans two chunks
    let data: Vec<u8> = (0..size).map(|i| (i % 191) as u8).collect();
    let mut app = app_with_bytes(&data);

    // Tombstone one byte in the second chunk, then export everything.
    app.cursor = 100_000;
    app.delete_current().unwrap();
    app.cursor = 0;
    app.toggle_visual();
    app.move_horizontal((app.document.len() - 1) as i64);

    let dir = tempdir().unwrap();
    let path = dir.path().join("stream_export_tombstone.bin");
    app.execute_command(Command::Export {
        format: ExportFormat::Binary { path: path.clone() },
    })
    .unwrap();

    let mut expected = data.clone();
    expected.remove(100_000);
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert!(app.status_message.contains("logical bytes"));
}

#[test]
fn fill_streams_repeating_pattern_across_chunk_boundary() {
    let size = 200_000usize;
    let mut app = app_with_bytes(&vec![0u8; size]);
    app.cursor = 0;
    app.execute_command(Command::Fill {
        pattern: vec![0xde, 0xad, 0xbe],
        len: size,
    })
    .unwrap();

    let filled = app.document.logical_bytes(0, (size - 1) as u64).unwrap();
    let pattern = [0xde, 0xad, 0xbe];
    for (i, byte) in filled.iter().enumerate() {
        assert_eq!(*byte, pattern[i % 3], "mismatch at {i}");
    }
    assert!(app.status_message.contains(&format!("filled {size} bytes")));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, (size - 1) as u64).unwrap(),
        vec![0u8; size]
    );
}

#[test]
fn xor_bang_streams_large_selection_across_chunk_boundary() {
    let size = 200_000usize;
    let data: Vec<u8> = (0..size).map(|i| (i % 253) as u8).collect();
    let mut app = app_with_bytes(&data);
    app.cursor = 0;
    app.toggle_visual();
    app.move_horizontal((app.document.len() - 1) as i64);

    app.execute_command(Command::Xor {
        key: 0x5a,
        in_place: true,
    })
    .unwrap();

    let xored = app.document.logical_bytes(0, (size - 1) as u64).unwrap();
    let expected: Vec<u8> = data.iter().map(|b| b ^ 0x5a).collect();
    assert_eq!(xored, expected);
    assert!(app
        .status_message
        .contains(&format!("replaced {size} logical bytes")));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, (size - 1) as u64).unwrap(),
        data
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Redo: visual delete and paste
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn redo_reapplies_various_actions() {
    // Redo visual delete
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.toggle_visual();
    app.move_horizontal(2);
    app.delete_at_cursor_or_selection().unwrap();
    app.undo(1, true).unwrap();
    app.redo(1, true).unwrap();
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Deleted);
}
