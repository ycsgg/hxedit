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
fn source_macro_runs_toml_steps_as_grouped_undo() {
    let dir = tempdir().unwrap();
    let macro_path = dir.path().join("patch.hxmacro");
    fs::write(
        &macro_path,
        r#"
version = 1
selection = "clear"
undo = "group"

[[steps]]
cmd = "fill"
offset = "0x1"
pattern = "aa bb"
len = 3

[[steps]]
cmd = "overwrite"
offset = "0x4"
bytes = "cc"
"#,
    )
    .unwrap();

    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13, 0x14]);
    app.execute_command(Command::Source {
        path: macro_path.clone(),
    })
    .unwrap();

    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0xaa, 0xbb, 0xaa, 0xcc]
    );
    assert_eq!(app.undo_stack.len(), 1);
    assert!(app.status_message.contains("ran 2 steps"));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0x11, 0x12, 0x13, 0x14]
    );
}

#[cfg(feature = "scripting")]
#[test]
fn script_command_runs_rhai_steps_as_grouped_undo() {
    let dir = tempdir().unwrap();
    let script_path = dir.path().join("patch.hxscript");
    fs::write(
        &script_path,
        r#"
hx_select_display(0, 4);
let digest = hx_hash_hex("crc32");
hx_overwrite(4, hx_ascii(digest));
hx_insert(0, hx_ascii("!"));
hx_fill(1, hx_hex("41"), 1);
"#,
    )
    .unwrap();

    let mut app = app_with_bytes(b"abcd........");
    app.execute_command(Command::Script {
        path: script_path.clone(),
    })
    .unwrap();

    let digest = crc32fast::hash(b"abcd").to_be_bytes();
    let mut expected = b"!Abcd".to_vec();
    expected.extend(
        digest
            .iter()
            .flat_map(|byte| format!("{byte:02x}").into_bytes()),
    );
    assert_eq!(app.document.logical_bytes(0, 12).unwrap(), expected);
    assert_eq!(app.undo_stack.len(), 1);
    assert!(app.status_message.contains("script"));
    assert!(app.status_message.contains("ran 5 calls"));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 11).unwrap(),
        b"abcd........".to_vec()
    );
}

#[cfg(not(feature = "scripting"))]
#[test]
fn script_command_reports_missing_scripting_feature() {
    let mut app = app_with_bytes(b"abcd");
    let err = app
        .execute_command(Command::Script {
            path: "patch.hxscript".into(),
        })
        .unwrap_err();
    assert!(err.to_string().contains("scripting feature"));
}

#[test]
fn source_macro_inherits_visual_selection() {
    let dir = tempdir().unwrap();
    let macro_path = dir.path().join("xor_selection.hxmacro");
    fs::write(
        &macro_path,
        r#"
version = 1
selection = "require"

[[steps]]
cmd = "xor"
scope = "selection"
key = "0xff"
in_place = true
"#,
    )
    .unwrap();

    let mut app = app_with_bytes(&[0x00, 0x11, 0x22, 0x33]);
    app.cursor = 1;
    app.toggle_visual();
    app.move_horizontal(1);

    app.execute_command(Command::Source { path: macro_path })
        .unwrap();

    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0x00, 0xee, 0xdd, 0x33]
    );
    assert_eq!(app.selection_range(), Some((1, 2)));

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 3).unwrap(),
        vec![0x00, 0x11, 0x22, 0x33]
    );
}

#[test]
fn source_macro_stop_error_keeps_successful_edits_undoable() {
    let dir = tempdir().unwrap();
    let macro_path = dir.path().join("partial.hxmacro");
    fs::write(
        &macro_path,
        r#"
version = 1
on_error = "stop"

[[steps]]
cmd = "overwrite"
offset = "0x1"
bytes = "aa"

[[steps]]
cmd = "insert"
offset = "0xffff"
bytes = "bb"
"#,
    )
    .unwrap();

    let mut app = app_with_bytes(&[0x10, 0x11, 0x12]);
    let err = app
        .execute_command(Command::Source { path: macro_path })
        .unwrap_err();
    assert!(err.to_string().contains("stopped after 1 steps"));

    assert_eq!(
        app.document.logical_bytes(0, 2).unwrap(),
        vec![0x10, 0xaa, 0x12]
    );
    assert_eq!(app.undo_stack.len(), 1);

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 2).unwrap(),
        vec![0x10, 0x11, 0x12]
    );
}

#[test]
fn source_macro_rollback_error_reverts_successful_edits() {
    let dir = tempdir().unwrap();
    let macro_path = dir.path().join("rollback.hxmacro");
    fs::write(
        &macro_path,
        r#"
version = 1
on_error = "rollback"

[[steps]]
cmd = "overwrite"
offset = "0x1"
bytes = "aa"

[[steps]]
cmd = "insert"
offset = "0xffff"
bytes = "bb"
"#,
    )
    .unwrap();

    let mut app = app_with_bytes(&[0x10, 0x11, 0x12]);
    let err = app
        .execute_command(Command::Source { path: macro_path })
        .unwrap_err();
    assert!(err.to_string().contains("stopped after 1 steps"));

    assert_eq!(
        app.document.logical_bytes(0, 2).unwrap(),
        vec![0x10, 0x11, 0x12]
    );
    assert!(app.undo_stack.is_empty());
}

#[test]
fn source_macro_per_step_undo_splits_steps() {
    let dir = tempdir().unwrap();
    let macro_path = dir.path().join("per_step.hxmacro");
    fs::write(
        &macro_path,
        r#"
version = 1
undo = "per-step"

[[steps]]
cmd = "overwrite"
offset = "0x1"
bytes = "aa"

[[steps]]
cmd = "overwrite"
offset = "0x2"
bytes = "bb"
"#,
    )
    .unwrap();

    let mut app = app_with_bytes(&[0x10, 0x11, 0x12]);
    app.execute_command(Command::Source { path: macro_path })
        .unwrap();

    assert_eq!(
        app.document.logical_bytes(0, 2).unwrap(),
        vec![0x10, 0xaa, 0xbb]
    );
    assert_eq!(app.undo_stack.len(), 2);

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 2).unwrap(),
        vec![0x10, 0xaa, 0x12]
    );
}

#[test]
fn source_macro_reuses_hash_result_in_later_write() {
    let dir = tempdir().unwrap();
    let macro_path = dir.path().join("hash_result.hxmacro");
    fs::write(
        &macro_path,
        r#"
version = 1

[[steps]]
cmd = "hash"
id = "payload_crc32"
algorithm = "crc32"
scope = { space = "display", start = "0x0", len = 4 }

[[steps]]
cmd = "insert"
offset = "0x4"
bytes = { from = "payload_crc32", format = "bytes" }

[[steps]]
cmd = "insert"
offset = "0x8"
bytes = { from = "payload_crc32", format = "hex-text" }
"#,
    )
    .unwrap();

    let mut app = app_with_bytes(b"abcd");
    app.execute_command(Command::Source { path: macro_path })
        .unwrap();

    let digest = crc32fast::hash(b"abcd").to_be_bytes();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut expected = b"abcd".to_vec();
    expected.extend_from_slice(&digest);
    expected.extend_from_slice(hex.as_bytes());
    assert_eq!(
        app.document
            .logical_bytes(0, app.document.len() - 1)
            .unwrap(),
        expected
    );
    assert_eq!(app.undo_stack.len(), 1);

    app.undo(1, true).unwrap();
    assert_eq!(
        app.document
            .logical_bytes(0, app.document.len() - 1)
            .unwrap(),
        b"abcd"
    );
}

#[derive(Clone, Debug)]
struct AppFuzzRng {
    state: u64,
}

impl AppFuzzRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn range(&mut self, upper_exclusive: usize) -> usize {
        if upper_exclusive == 0 {
            return 0;
        }
        (self.next_u64() as usize) % upper_exclusive
    }

    fn byte(&mut self) -> u8 {
        (self.next_u64() >> 32) as u8
    }
}

#[derive(Clone, Debug)]
struct AppFuzzModel {
    slots: Vec<Option<u8>>,
}

impl AppFuzzModel {
    fn new(bytes: &[u8]) -> Self {
        Self {
            slots: bytes.iter().copied().map(Some).collect(),
        }
    }

    fn logical_bytes(&self) -> Vec<u8> {
        self.slots.iter().filter_map(|slot| *slot).collect()
    }

    fn assert_matches(&self, app: &mut App, step: usize, label: &str) {
        assert_eq!(
            app.document.len() as usize,
            self.slots.len(),
            "step {step} {label}: display len"
        );

        let logical = self.logical_bytes();
        assert_eq!(
            app.document.visible_len() as usize,
            logical.len(),
            "step {step} {label}: visible len"
        );

        if self.slots.is_empty() {
            assert_eq!(
                app.document.byte_at(0).unwrap(),
                ByteSlot::Empty,
                "step {step} {label}: empty EOF slot"
            );
            return;
        }

        assert_eq!(
            app.document
                .logical_bytes(0, app.document.len() - 1)
                .unwrap(),
            logical,
            "step {step} {label}: logical bytes"
        );
        assert_eq!(
            app.document
                .logical_byte_count(0, app.document.len() - 1)
                .unwrap(),
            logical.len() as u64,
            "step {step} {label}: logical byte count"
        );

        let mut logical_offset = 0_u64;
        for (display_offset, expected) in self.slots.iter().enumerate() {
            let display_offset = display_offset as u64;
            match expected {
                Some(value) => {
                    assert_eq!(
                        app.document.byte_at(display_offset).unwrap(),
                        ByteSlot::Present(*value),
                        "step {step} {label}: display slot {display_offset}"
                    );
                    assert_eq!(
                        app.document
                            .logical_offset_for_display_offset(display_offset),
                        Some(logical_offset),
                        "step {step} {label}: display->logical {display_offset}"
                    );
                    assert_eq!(
                        app.document
                            .display_offset_for_logical_offset(logical_offset),
                        Some(display_offset),
                        "step {step} {label}: logical->display {logical_offset}"
                    );
                    logical_offset += 1;
                }
                None => {
                    assert_eq!(
                        app.document.byte_at(display_offset).unwrap(),
                        ByteSlot::Deleted,
                        "step {step} {label}: tombstone slot {display_offset}"
                    );
                    assert_eq!(
                        app.document
                            .logical_offset_for_display_offset(display_offset),
                        None,
                        "step {step} {label}: tombstone logical offset"
                    );
                }
            }
        }

        assert_eq!(
            app.document.byte_at(app.document.len()).unwrap(),
            ByteSlot::Empty,
            "step {step} {label}: EOF slot"
        );
    }

    fn present_span(&self, rng: &mut AppFuzzRng, max_len: usize) -> Option<(usize, usize)> {
        let candidates = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(start, slot)| {
                slot.as_ref()?;
                let available = self.slots[start..]
                    .iter()
                    .take_while(|slot| slot.is_some())
                    .count();
                Some((start, available.min(max_len.max(1))))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        let (start, available) = candidates[rng.range(candidates.len())];
        Some((start, 1 + rng.range(available)))
    }

    fn display_span(&self, rng: &mut AppFuzzRng, max_len: usize) -> Option<(usize, usize)> {
        if self.slots.is_empty() {
            return None;
        }
        let start = rng.range(self.slots.len());
        let available = (self.slots.len() - start).min(max_len.max(1));
        Some((start, 1 + rng.range(available)))
    }

    fn overwrite(&mut self, offset: usize, bytes: &[u8]) {
        for (slot, byte) in self.slots[offset..offset + bytes.len()]
            .iter_mut()
            .zip(bytes)
        {
            assert!(slot.is_some());
            *slot = Some(*byte);
        }
    }

    fn insert(&mut self, offset: usize, bytes: &[u8]) {
        self.slots
            .splice(offset..offset, bytes.iter().copied().map(Some));
    }

    fn tombstone_delete(&mut self, offset: usize, len: usize) {
        for slot in &mut self.slots[offset..offset + len] {
            if slot.is_some() {
                *slot = None;
            }
        }
    }

    fn real_delete(&mut self, offset: usize, len: usize) {
        self.slots.drain(offset..offset + len);
    }

    fn fill(&mut self, offset: usize, len: usize, pattern: &[u8]) {
        for (index, slot) in self.slots[offset..offset + len].iter_mut().enumerate() {
            if slot.is_some() {
                *slot = Some(pattern[index % pattern.len()]);
            }
        }
    }

    fn xor(&mut self, offset: usize, len: usize, key: u8) {
        for value in self.slots[offset..offset + len].iter_mut().flatten() {
            *value ^= key;
        }
    }
}

fn fuzz_bytes(rng: &mut AppFuzzRng, count: usize) -> Vec<u8> {
    (0..count).map(|_| rng.byte()).collect()
}

fn fuzz_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn clear_app_fuzz_selection(app: &mut App) {
    app.selection_anchor = None;
    app.mode = Mode::Normal;
    app.cursor = app.clamp_cursor_for_mode(app.cursor, Mode::Normal);
}

fn select_app_fuzz_range(app: &mut App, start: usize, len: usize) {
    let end = start + len - 1;
    app.cursor = start as u64;
    app.toggle_visual();
    app.cursor = end as u64;
}

fn assert_app_fuzz_undo_redo(
    app: &mut App,
    before: &AppFuzzModel,
    after: &AppFuzzModel,
    undo_before: usize,
    step: usize,
) {
    let added_steps = app.undo_stack.len().saturating_sub(undo_before);
    after.assert_matches(app, step, "after op");

    if added_steps > 0 && step.is_multiple_of(7) {
        app.undo(added_steps, true).unwrap();
        before.assert_matches(app, step, "after undo");
        app.redo(added_steps, true).unwrap();
        after.assert_matches(app, step, "after redo");
    }

    clear_app_fuzz_selection(app);
}

#[test]
fn source_macro_mixed_with_normal_edits_fuzz_matches_reference_model() {
    let dir = tempdir().unwrap();
    let macro_path = dir.path().join("mixed_fuzz.hxmacro");
    let initial = (0..48).map(|index| index as u8).collect::<Vec<_>>();
    let mut app = app_with_bytes(&initial);
    let mut model = AppFuzzModel::new(&initial);
    let mut rng = AppFuzzRng::new(0xa11c_e5ed_5eed_f00d);

    model.assert_matches(&mut app, 0, "initial");

    for step in 0..320 {
        if model.slots.is_empty() {
            let count = 1 + rng.range(3);
            let bytes = fuzz_bytes(&mut rng, count);
            app.cursor = 0;
            app.apply_paste_insert(&bytes).unwrap();
            model.insert(0, &bytes);
            model.assert_matches(&mut app, step, "refill empty");
            clear_app_fuzz_selection(&mut app);
            continue;
        }

        clear_app_fuzz_selection(&mut app);
        let before = model.clone();
        let undo_before = app.undo_stack.len();

        match rng.range(10) {
            0 => {
                if let Some((offset, len)) = model.present_span(&mut rng, 4) {
                    let bytes = fuzz_bytes(&mut rng, len);
                    app.cursor = offset as u64;
                    assert_eq!(app.apply_paste_overwrite(&bytes).unwrap(), len);
                    model.overwrite(offset, &bytes);
                }
            }
            1 => {
                if model.slots.len() < 160 {
                    let offset = rng.range(model.slots.len() + 1);
                    let count = 1 + rng.range(4);
                    let bytes = fuzz_bytes(&mut rng, count);
                    app.cursor = offset as u64;
                    assert_eq!(app.apply_paste_insert(&bytes).unwrap(), bytes.len());
                    model.insert(offset, &bytes);
                }
            }
            2 => {
                let offset = rng.range(model.slots.len());
                app.cursor = offset as u64;
                app.delete_at_cursor_or_selection().unwrap();
                model.tombstone_delete(offset, 1);
            }
            3 => {
                if let Some((offset, len)) = model.present_span(&mut rng, 8) {
                    let pattern_len = 1 + rng.range(3);
                    let pattern = fuzz_bytes(&mut rng, pattern_len);
                    app.cursor = offset as u64;
                    app.execute_command(Command::Fill {
                        pattern: pattern.clone(),
                        len,
                    })
                    .unwrap();
                    model.fill(offset, len, &pattern);
                }
            }
            4 => {
                if let Some((offset, len)) = model.display_span(&mut rng, 8) {
                    let key = rng.byte() | 1;
                    select_app_fuzz_range(&mut app, offset, len);
                    app.execute_command(Command::Xor {
                        key,
                        in_place: true,
                    })
                    .unwrap();
                    model.xor(offset, len, key);
                }
            }
            5 => {
                if model.slots.len() < 160 {
                    let (overwrite_offset, overwrite_len) =
                        model.present_span(&mut rng, 4).unwrap_or((0, 0));
                    let overwrite_bytes = fuzz_bytes(&mut rng, overwrite_len);
                    let insert_offset = rng.range(model.slots.len() + 1);
                    let insert_len = 1 + rng.range(4);
                    let insert_bytes = fuzz_bytes(&mut rng, insert_len);

                    fs::write(
                        &macro_path,
                        format!(
                            r#"
version = 1
selection = "clear"

[[steps]]
cmd = "overwrite"
offset = "0x{overwrite_offset:x}"
bytes = "{}"

[[steps]]
cmd = "insert"
offset = "0x{insert_offset:x}"
bytes = "{}"
"#,
                            fuzz_hex(&overwrite_bytes),
                            fuzz_hex(&insert_bytes)
                        ),
                    )
                    .unwrap();
                    app.execute_command(Command::Source {
                        path: macro_path.clone(),
                    })
                    .unwrap();
                    model.overwrite(overwrite_offset, &overwrite_bytes);
                    model.insert(insert_offset, &insert_bytes);
                }
            }
            6 => {
                if let Some((offset, len)) = model.display_span(&mut rng, 6) {
                    fs::write(
                        &macro_path,
                        format!(
                            r#"
version = 1
selection = "clear"

[[steps]]
cmd = "delete"
scope = {{ space = "display", start = "0x{offset:x}", len = {len} }}
kind = "tombstone"
"#
                        ),
                    )
                    .unwrap();
                    app.execute_command(Command::Source {
                        path: macro_path.clone(),
                    })
                    .unwrap();
                    model.tombstone_delete(offset, len);
                }
            }
            7 => {
                if let Some((offset, len)) = model.display_span(&mut rng, 4) {
                    fs::write(
                        &macro_path,
                        format!(
                            r#"
version = 1
selection = "clear"

[[steps]]
cmd = "delete"
scope = {{ space = "display", start = "0x{offset:x}", len = {len} }}
kind = "real"
"#
                        ),
                    )
                    .unwrap();
                    app.execute_command(Command::Source {
                        path: macro_path.clone(),
                    })
                    .unwrap();
                    model.real_delete(offset, len);
                }
            }
            8 => {
                if let Some((offset, len)) = model.present_span(&mut rng, 8) {
                    let pattern_len = 1 + rng.range(3);
                    let pattern = fuzz_bytes(&mut rng, pattern_len);
                    fs::write(
                        &macro_path,
                        format!(
                            r#"
version = 1
selection = "clear"

[[steps]]
cmd = "fill"
offset = "0x{offset:x}"
pattern = "{}"
len = {len}
"#,
                            fuzz_hex(&pattern)
                        ),
                    )
                    .unwrap();
                    app.execute_command(Command::Source {
                        path: macro_path.clone(),
                    })
                    .unwrap();
                    model.fill(offset, len, &pattern);
                }
            }
            _ => {
                if let Some((offset, len)) = model.display_span(&mut rng, 8) {
                    let key = rng.byte() | 1;
                    select_app_fuzz_range(&mut app, offset, len);
                    fs::write(
                        &macro_path,
                        format!(
                            r#"
version = 1
selection = "require"

[[steps]]
cmd = "xor"
scope = "selection"
key = "0x{key:02x}"
in_place = true
"#
                        ),
                    )
                    .unwrap();
                    app.execute_command(Command::Source {
                        path: macro_path.clone(),
                    })
                    .unwrap();
                    model.xor(offset, len, key);
                }
            }
        }

        assert_app_fuzz_undo_redo(&mut app, &before, &model, undo_before, step);
    }
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
