use std::fs;

use tempfile::tempdir;

use crate::app::{App, SearchDirection, StatusLevel};
use crate::cli::Cli;
use crate::commands::types::{Command, ExportFormat, GotoTarget, HashAlgorithm};
use crate::core::document::ByteSlot;
use crate::format::parse::{FieldValue, StructValue};
use crate::format::types::{FieldDef, FieldType};
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

fn app_with_inspector_field(bytes: &[u8], offset: u64, size: usize) -> App {
    let mut app = app_with_bytes(bytes);
    let field = FieldDef {
        name: "field".to_owned(),
        offset,
        field_type: FieldType::Bytes(size),
        description: String::new(),
        editable: false,
    };
    let structs = vec![StructValue {
        name: "Header".to_owned(),
        base_offset: 0,
        fields: vec![FieldValue {
            def: field,
            abs_offset: offset,
            raw_bytes: bytes[offset as usize..offset as usize + size].to_vec(),
            display: format!("{} bytes", size),
            size,
        }],
        children: Vec::new(),
    }];
    let collapsed_nodes = std::collections::BTreeSet::new();
    let rows = crate::format::parse::flatten(&structs, &collapsed_nodes);
    app.show_inspector = true;
    app.inspector = Some(crate::app::InspectorState {
        format_name: "TEST".to_owned(),
        structs,
        rows,
        scroll_offset: 0,
        selected_row: 1,
        editing: None,
        collapsed_nodes,
    });
    app.mode = Mode::Inspector;
    app.cursor = offset;
    app
}

fn build_disassembly_elf64(code: &[u8]) -> Vec<u8> {
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
    bytes[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
    bytes[0x100..0x100 + code.len()].copy_from_slice(code);
    bytes
}

fn build_paginated_elf64(section_count: usize) -> Vec<u8> {
    const EHDR_SIZE: usize = 64;
    const PHDR_OFFSET: usize = EHDR_SIZE;
    const PHDR_SIZE: usize = 56;
    const SHDR_OFFSET: usize = 0x200;
    const SHDR_SIZE: usize = 64;
    const SHSTRTAB_OFFSET: usize = 0x120;
    const TEXT_OFFSET: usize = 0x100;

    fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
        buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
        buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64_le(buf: &mut [u8], offset: usize, value: u64) {
        buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    let names: Vec<String> = std::iter::once(".shstrtab".to_owned())
        .chain(std::iter::once(".text".to_owned()))
        .chain((0..section_count.saturating_sub(3)).map(|idx| format!(".extra_{idx}")))
        .collect();

    let mut strtab = vec![0_u8];
    let mut name_offsets = Vec::with_capacity(names.len());
    for name in &names {
        let start = strtab.len() as u32;
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
        name_offsets.push(start);
    }

    let total_sections = 1 + names.len();
    let total_len = SHDR_OFFSET + total_sections * SHDR_SIZE;
    let mut bytes = vec![0_u8; total_len.max(SHSTRTAB_OFFSET + strtab.len())];

    bytes[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;

    write_u16_le(&mut bytes, 16, 2);
    write_u16_le(&mut bytes, 18, 0x3e);
    write_u32_le(&mut bytes, 20, 1);
    write_u64_le(&mut bytes, 24, 0x401000);
    write_u64_le(&mut bytes, 32, PHDR_OFFSET as u64);
    write_u64_le(&mut bytes, 40, SHDR_OFFSET as u64);
    write_u16_le(&mut bytes, 52, EHDR_SIZE as u16);
    write_u16_le(&mut bytes, 54, PHDR_SIZE as u16);
    write_u16_le(&mut bytes, 56, 1);
    write_u16_le(&mut bytes, 58, SHDR_SIZE as u16);
    write_u16_le(&mut bytes, 60, total_sections as u16);
    write_u16_le(&mut bytes, 62, 1);

    write_u32_le(&mut bytes, PHDR_OFFSET, 1);
    write_u32_le(&mut bytes, PHDR_OFFSET + 4, 0x5);
    write_u64_le(&mut bytes, PHDR_OFFSET + 8, TEXT_OFFSET as u64);
    write_u64_le(&mut bytes, PHDR_OFFSET + 16, 0x401000);
    write_u64_le(&mut bytes, PHDR_OFFSET + 24, 0x401000);
    write_u64_le(&mut bytes, PHDR_OFFSET + 32, 4);
    write_u64_le(&mut bytes, PHDR_OFFSET + 40, 4);
    write_u64_le(&mut bytes, PHDR_OFFSET + 48, 0x1000);

    bytes[TEXT_OFFSET..TEXT_OFFSET + 4].copy_from_slice(&[0x90, 0x90, 0x90, 0xc3]);
    bytes[SHSTRTAB_OFFSET..SHSTRTAB_OFFSET + strtab.len()].copy_from_slice(&strtab);

    let shstrtab = SHDR_OFFSET + SHDR_SIZE;
    write_u32_le(&mut bytes, shstrtab, name_offsets[0]);
    write_u32_le(&mut bytes, shstrtab + 4, 3);
    write_u64_le(&mut bytes, shstrtab + 24, SHSTRTAB_OFFSET as u64);
    write_u64_le(&mut bytes, shstrtab + 32, strtab.len() as u64);
    write_u64_le(&mut bytes, shstrtab + 48, 1);

    let text = SHDR_OFFSET + SHDR_SIZE * 2;
    write_u32_le(&mut bytes, text, name_offsets[1]);
    write_u32_le(&mut bytes, text + 4, 1);
    write_u64_le(&mut bytes, text + 8, 0x6);
    write_u64_le(&mut bytes, text + 16, 0x401000);
    write_u64_le(&mut bytes, text + 24, TEXT_OFFSET as u64);
    write_u64_le(&mut bytes, text + 32, 4);
    write_u64_le(&mut bytes, text + 48, 16);

    for (idx, name_offset) in name_offsets.iter().enumerate().skip(2) {
        let header = SHDR_OFFSET + SHDR_SIZE * (idx + 1);
        write_u32_le(&mut bytes, header, *name_offset);
        write_u32_le(&mut bytes, header + 4, 1);
        write_u64_le(&mut bytes, header + 8, 0x2);
        write_u64_le(&mut bytes, header + 48, 1);
    }

    bytes
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
fn inspector_jump_centers_target_row_in_hex_view() {
    let bytes = vec![0_u8; 256];
    let mut app = app_with_inspector_field(&bytes, 160, 1);
    app.cursor = 0;
    app.viewport_top = 0;

    app.sync_cursor_to_inspector();

    assert_eq!(app.cursor, 160);
    assert_eq!(app.viewport_top, 128);
}

#[test]
fn inspector_jump_keeps_viewport_when_target_is_already_visible() {
    let bytes = vec![0_u8; 256];
    let mut app = app_with_inspector_field(&bytes, 160, 1);
    app.cursor = 0;
    app.viewport_top = 128;

    app.sync_cursor_to_inspector();

    assert_eq!(app.cursor, 160);
    assert_eq!(app.viewport_top, 128);
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
fn inspector_more_detects_nested_elf_pagination_markers() {
    let mut app = app_with_bytes(&build_paginated_elf64(70));
    app.show_inspector = true;
    app.inspector_format_override = Some("elf".to_owned());
    app.inspector_entry_cap = 1;
    app.refresh_inspector();

    app.execute_command(Command::InspectorMore).unwrap();

    assert_eq!(app.status_level, StatusLevel::Info);
    assert!(app.status_message.contains("more entries still pending"));
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
fn goto_command_reports_moved_delta() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.cursor = 1;

    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(2),
    })
    .unwrap();
    assert!(app.status_message.contains("moved +0x2"));
    assert!(app.status_message.contains("→ 0x3"));

    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(-1),
    })
    .unwrap();
    assert!(app.status_message.contains("moved -0x1"));
    assert!(app.status_message.contains("→ 0x2"));
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
fn export_command_uses_selected_inspector_field_range() {
    let mut app = app_with_inspector_field(b"hello world", 6, 5);
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.bin");

    app.execute_command(Command::Export {
        format: ExportFormat::Binary { path: path.clone() },
    })
    .unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"world");
}

#[test]
fn replace_command_overwrites_all_equal_length_matches() {
    let mut app = app_with_bytes(b"abcabc");
    app.execute_command(Command::Replace {
        needle: b"ab".to_vec(),
        replacement: b"xy".to_vec(),
        allow_resize: false,
    })
    .unwrap();

    assert_eq!(app.document.len(), 6);
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"xycxyc");
    assert!(app.status_message.contains("replaced 2 matches"));

    app.undo(1, true).unwrap();
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"abcabc");
}

#[test]
fn replace_bang_can_resize_matches() {
    let mut app = app_with_bytes(b"abcabc");
    app.execute_command(Command::Replace {
        needle: b"ab".to_vec(),
        replacement: b"Z".to_vec(),
        allow_resize: true,
    })
    .unwrap();

    assert_eq!(app.document.len(), 4);
    assert_eq!(app.document.logical_bytes(0, 3).unwrap(), b"ZcZc");
    assert!(app.status_message.contains("4→2 bytes"));

    app.undo(1, true).unwrap();
    assert_eq!(app.document.len(), 6);
    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"abcabc");
}

#[test]
fn replace_command_respects_visual_selection_scope() {
    let mut app = app_with_bytes(b"abxxab");
    app.toggle_visual();
    app.move_horizontal(3);

    app.execute_command(Command::Replace {
        needle: b"ab".to_vec(),
        replacement: b"xy".to_vec(),
        allow_resize: false,
    })
    .unwrap();

    assert_eq!(app.document.logical_bytes(0, 5).unwrap(), b"xyxxab");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_range(), None);
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
fn hash_command_on_selected_inspector_field_uses_field_range() {
    let mut app = app_with_inspector_field(b"hello world", 6, 5);
    app.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();

    assert!(app.status_message.contains("sel 0x6-0xa"));
    assert!(app.status_message.contains("486ea46224d1bb4f"));
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

#[test]
fn disassemble_command_switches_main_view() {
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
}

#[test]
fn disassemble_command_aligns_viewport_to_containing_instruction() {
    let bytes = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.cursor = 0x102;

    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    assert_eq!(app.cursor, 0x102);
    match &app.main_view {
        crate::app::MainView::Disassembly(state) => assert_eq!(state.viewport_top, 0x101),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
}

#[test]
fn disassemble_off_returns_to_hex_view() {
    let bytes = build_disassembly_elf64(&[0x90, 0x90, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.execute_command(Command::DisassembleOff).unwrap();
    assert!(matches!(app.main_view, crate::app::MainView::Hex));
}

#[test]
fn disassembly_vertical_move_uses_instruction_boundaries() {
    let bytes = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    assert_eq!(app.cursor, 0x100);
    app.move_vertical(1);
    assert_eq!(app.cursor, 0x101);
    app.move_vertical(2);
    assert_eq!(app.cursor, 0x105);
    app.move_vertical(-1);
    assert_eq!(app.cursor, 0x104);
}

#[test]
fn disassembly_scroll_viewport_uses_instruction_rows() {
    let bytes = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.view_rows = 2;
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.scroll_viewport(1);
    match &app.main_view {
        crate::app::MainView::Disassembly(state) => assert_eq!(state.viewport_top, 0x101),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
    assert_eq!(app.cursor, 0x101);

    app.scroll_viewport(1);
    match &app.main_view {
        crate::app::MainView::Disassembly(state) => assert_eq!(state.viewport_top, 0x104),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
    assert_eq!(app.cursor, 0x104);
}

#[test]
fn disassembly_scroll_up_from_raw_tail_does_not_snap_back_to_text_end() {
    let bytes = build_disassembly_elf64(&[0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.view_rows = 2;
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.scroll_viewport(99);
    let bottom_top = match &app.main_view {
        crate::app::MainView::Disassembly(state) => state.viewport_top,
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    };
    assert!(bottom_top >= 0x1f0);

    app.scroll_viewport(-1);
    match &app.main_view {
        crate::app::MainView::Disassembly(state) => {
            assert_eq!(state.viewport_top, bottom_top.saturating_sub(8));
            assert!(state.viewport_top > 0x102);
        }
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
}

#[test]
fn instruction_search_jumps_to_matching_instruction_row() {
    let bytes = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.view_rows = 4;
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.execute_command(Command::SearchInstruction {
        pattern: "ret".to_owned(),
        backward: false,
    })
    .unwrap();

    assert_eq!(app.cursor, 0x105);
    match &app.main_view {
        crate::app::MainView::Disassembly(state) => assert_eq!(state.viewport_top, 0x105),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
}

#[test]
fn byte_search_in_disassembly_recenters_to_containing_instruction_row() {
    let bytes = build_disassembly_elf64(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
    let mut app = app_with_bytes(&bytes);
    app.view_rows = 4;
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();

    app.execute_command(Command::SearchHex {
        pattern: vec![0x89, 0xe5],
        backward: false,
    })
    .unwrap();

    assert_eq!(app.cursor, 0x102);
    match &app.main_view {
        crate::app::MainView::Disassembly(state) => assert_eq!(state.viewport_top, 0x101),
        crate::app::MainView::Hex => panic!("expected disassembly view"),
    }
}
