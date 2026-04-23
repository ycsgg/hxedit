//! Consolidated tests for App-level functionality.
//!
//! Tests are grouped by functionality and consolidated where possible
//! to reduce total count while maintaining coverage.

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

fn build_disassembly_elf64_with_symbol(code: &[u8], symbol_name: &str) -> Vec<u8> {
    let text_offset = 0x100usize;
    let text_addr = 0x401000u64;
    let strtab_offset = 0x120usize;
    let mut strtab = vec![0_u8];
    let symbol_name_offset = strtab.len() as u32;
    strtab.extend_from_slice(symbol_name.as_bytes());
    strtab.push(0);

    let symtab_offset = 0x140usize;
    let shstr_offset = 0x180usize;
    let mut shstr = vec![0_u8];
    let text_name = shstr.len() as u32;
    shstr.extend_from_slice(b".text\0");
    let strtab_name = shstr.len() as u32;
    shstr.extend_from_slice(b".strtab\0");
    let symtab_name = shstr.len() as u32;
    shstr.extend_from_slice(b".symtab\0");
    let shstr_name = shstr.len() as u32;
    shstr.extend_from_slice(b".shstrtab\0");

    let shoff = 0x200usize;
    let mut bytes = vec![0_u8; shoff + 5 * 64];
    bytes[0..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x100u64.to_le_bytes());
    bytes[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
    bytes[60..62].copy_from_slice(&5u16.to_le_bytes());
    bytes[62..64].copy_from_slice(&4u16.to_le_bytes());

    bytes[text_offset..text_offset + code.len()].copy_from_slice(code);
    bytes[strtab_offset..strtab_offset + strtab.len()].copy_from_slice(&strtab);
    bytes[shstr_offset..shstr_offset + shstr.len()].copy_from_slice(&shstr);

    let mut symtab = vec![0_u8; 48];
    let base = 24usize;
    symtab[base..base + 4].copy_from_slice(&symbol_name_offset.to_le_bytes());
    symtab[base + 4] = 0x12;
    symtab[base + 6..base + 8].copy_from_slice(&1u16.to_le_bytes());
    symtab[base + 8..base + 16].copy_from_slice(&text_addr.to_le_bytes());
    symtab[base + 16..base + 24].copy_from_slice(&(code.len() as u64).to_le_bytes());
    bytes[symtab_offset..symtab_offset + symtab.len()].copy_from_slice(&symtab);

    struct ShdrSpec {
        index: usize,
        name: u32,
        sh_type: u32,
        flags: u64,
        addr: u64,
        offset: u64,
        size: u64,
        link: u32,
        info: u32,
        addralign: u64,
        entsize: u64,
    }

    fn write_shdr(bytes: &mut [u8], spec: ShdrSpec) {
        let base = spec.index * 64;
        bytes[base..base + 4].copy_from_slice(&spec.name.to_le_bytes());
        bytes[base + 4..base + 8].copy_from_slice(&spec.sh_type.to_le_bytes());
        bytes[base + 8..base + 16].copy_from_slice(&spec.flags.to_le_bytes());
        bytes[base + 16..base + 24].copy_from_slice(&spec.addr.to_le_bytes());
        bytes[base + 24..base + 32].copy_from_slice(&spec.offset.to_le_bytes());
        bytes[base + 32..base + 40].copy_from_slice(&spec.size.to_le_bytes());
        bytes[base + 40..base + 44].copy_from_slice(&spec.link.to_le_bytes());
        bytes[base + 44..base + 48].copy_from_slice(&spec.info.to_le_bytes());
        bytes[base + 48..base + 56].copy_from_slice(&spec.addralign.to_le_bytes());
        bytes[base + 56..base + 64].copy_from_slice(&spec.entsize.to_le_bytes());
    }

    write_shdr(
        &mut bytes[shoff..shoff + 5 * 64],
        ShdrSpec {
            index: 1,
            name: text_name,
            sh_type: 1,
            flags: 0x6,
            addr: text_addr,
            offset: text_offset as u64,
            size: code.len() as u64,
            link: 0,
            info: 0,
            addralign: 16,
            entsize: 0,
        },
    );
    write_shdr(
        &mut bytes[shoff..shoff + 5 * 64],
        ShdrSpec {
            index: 2,
            name: strtab_name,
            sh_type: 3,
            flags: 0,
            addr: 0,
            offset: strtab_offset as u64,
            size: strtab.len() as u64,
            link: 0,
            info: 0,
            addralign: 1,
            entsize: 0,
        },
    );
    write_shdr(
        &mut bytes[shoff..shoff + 5 * 64],
        ShdrSpec {
            index: 3,
            name: symtab_name,
            sh_type: 2,
            flags: 0,
            addr: 0,
            offset: symtab_offset as u64,
            size: symtab.len() as u64,
            link: 2,
            info: 1,
            addralign: 8,
            entsize: 24,
        },
    );
    write_shdr(
        &mut bytes[shoff..shoff + 5 * 64],
        ShdrSpec {
            index: 4,
            name: shstr_name,
            sh_type: 3,
            flags: 0,
            addr: 0,
            offset: shstr_offset as u64,
            size: shstr.len() as u64,
            link: 0,
            info: 0,
            addralign: 1,
            entsize: 0,
        },
    );

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

    // Stops at last page
    app.scroll_viewport(99);
    assert_eq!(app.viewport_top, 192);
}

// ═══════════════════════════════════════════════════════════════════════════
// Inspector: sync, jump, and pagination
// ═══════════════════════════════════════════════════════════════════════════

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
    app2.show_inspector = true;
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

// ═══════════════════════════════════════════════════════════════════════════
// Search: forward, backward, wrap
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn search_forward_backward_and_wrap() {
    let mut app = app_with_bytes(b"abc hello xyz hello end");

    // Forward search finds first match
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 4);

    // Search next and prev follow last pattern
    app.repeat_search(SearchDirection::Forward).unwrap();
    assert_eq!(app.cursor, 14);
    app.repeat_search(SearchDirection::Backward).unwrap();
    assert_eq!(app.cursor, 4);

    // Reverse search searches upward
    app.cursor = app.document.len() - 1;
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();
    assert_eq!(app.cursor, 14);
    app.repeat_search(SearchDirection::Backward).unwrap();
    assert_eq!(app.cursor, 4);

    // Forward search wraps to start
    let mut app2 = app_with_bytes(b"hello world");
    app2.cursor = app2.document.len() - 1;
    app2.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app2.cursor, 0);
    assert!(app2.status_message.contains("wrapped"));

    // Backward search wraps to end
    let mut app3 = app_with_bytes(b"hello world hello");
    app3.cursor = 0;
    app3.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();
    assert_eq!(app3.cursor, 12);
    assert!(app3.status_message.contains("wrapped"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Goto command: end, relative offsets, delta reporting
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn goto_command_various_targets() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);

    // End
    app.execute_command(Command::Goto {
        target: GotoTarget::End,
    })
    .unwrap();
    assert_eq!(app.cursor, 3);

    // Relative negative
    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(-2),
    })
    .unwrap();
    assert_eq!(app.cursor, 1);

    // Relative positive
    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(2),
    })
    .unwrap();
    assert_eq!(app.cursor, 3);

    // Delta reporting
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
}

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
    })
    .unwrap();
    assert_eq!(app3.document.logical_bytes(0, 5).unwrap(), b"xyxxab");
    assert_eq!(app3.mode, Mode::Normal);
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

// ═══════════════════════════════════════════════════════════════════════════
// Hash command: various algorithms and ranges
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hash_command_various_algorithms_and_ranges() {
    // SHA256 on entire file
    let mut app = app_with_bytes(b"hello");
    app.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app.status_message.contains("sha256"));
    assert!(app.status_message.contains("entire file"));
    assert!(app.status_message.contains("2cf24dba5fb0a30e"));

    // CRC32
    let mut app2 = app_with_bytes(b"hello");
    app2.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Crc32,
    })
    .unwrap();
    assert!(app2.status_message.contains("crc32"));

    // Visual selection
    let mut app3 = app_with_bytes(b"hello world");
    app3.toggle_visual();
    app3.move_horizontal(4);
    app3.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Md5,
    })
    .unwrap();
    assert!(app3.status_message.contains("md5"));
    assert!(app3.status_message.contains("sel 0x"));

    // Inspector field
    let mut app4 = app_with_inspector_field(b"hello world", 6, 5);
    app4.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app4.status_message.contains("sel 0x6-0xa"));
    assert!(app4.status_message.contains("486ea46224d1bb4f"));

    // Empty file
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
    let mut app5 = App::from_cli(cli).unwrap();
    app5.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app5.status_message.contains("no data"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Disassembly: view switch, viewport alignment, symbols
// ═══════════════════════════════════════════════════════════════════════════

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
    })
    .unwrap();
    assert_eq!(app2.cursor, 0x102);
}

// ═══════════════════════════════════════════════════════════════════════════
// Disassembly view editing: nibble edit, undo, redo, fill, replace
// ═══════════════════════════════════════════════════════════════════════════

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
    });
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("overwrite-only"));
}
