//! Consolidated tests for App-level functionality.
//!
//! Tests are grouped by functionality and consolidated where possible
//! to reduce total count while maintaining coverage.

use std::fs;
use std::io::{Seek, SeekFrom, Write};

use tempfile::tempdir;

use crate::action::Action;
use crate::app::{App, BulkReplacement, EditOp, SearchDirection, StatusLevel};
use crate::cli::Cli;
use crate::clipboard::test_clipboard_text;
use crate::commands::types::{Command, DiffCommand, ExportFormat, GotoTarget, HashAlgorithm};
use crate::config::Config;
use crate::core::document::{ByteSlot, Document};
use crate::format::parse::{FieldValue, StructValue};
use crate::format::types::{FieldDef, FieldType};
use crate::mode::{Mode, NibblePhase};

fn app_with_len(len: usize) -> App {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, vec![0_u8; len]).unwrap();
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
    app.view_rows = 4;
    app
}

fn app_with_bytes(bytes: &[u8]) -> App {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, bytes).unwrap();
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
    app.show_side_panel = true;
    app.active_side_panel = crate::app::SidePanelKind::Inspector;
    app.inspector_state = Some(crate::app::InspectorState {
        format_name: "TEST".to_owned(),
        structs,
        rows,
        scroll_offset: 0,
        selected_row: 1,
        editing: None,
        collapsed_nodes,
    });
    app.mode = Mode::SidePanel;
    app.cursor = offset;
    app
}

fn app_with_fixed_size_bytes(bytes: &[u8]) -> App {
    let mut app = app_with_bytes(bytes);
    let base = 0x1000_u64;
    app.document = Document::from_memory_bytes(
        format!("memory://4242/0x{base:x}-0x{:x}", base + bytes.len() as u64).into(),
        bytes.to_vec(),
        &Config::default(),
    );
    app.cursor = app.clamp_cursor_for_mode(app.cursor, app.mode);
    app
}

#[cfg(feature = "sagitta-analysis")]
fn sagitta_test_snapshot() -> super::analysis_state::SagittaSnapshot {
    sagitta_test_snapshot_for(0x401000, Some(1), "sub_401000")
}

#[cfg(feature = "sagitta-analysis")]
fn sagitta_test_snapshot_for(
    entry_va: u64,
    entry_logical_offset: Option<u64>,
    name: &str,
) -> super::analysis_state::SagittaSnapshot {
    use super::analysis_state::{
        RecoveredConfidence, RecoveredFunction, SagittaSnapshot, SagittaSummary,
    };

    SagittaSnapshot::new(
        SagittaSummary {
            functions: 1,
            blocks: 0,
            cfg_edges: 0,
            call_edges: 0,
            diagnostics: 0,
        },
        vec![RecoveredFunction {
            entry_va,
            entry_logical_offset,
            name: name.to_owned(),
            name_kind: crate::app::symbol_state::SymbolNameKind::Synthetic,
            confidence: RecoveredConfidence::Heuristic,
            provenance: Vec::new(),
            blocks: Vec::new(),
            callers: Vec::new(),
            callees: Vec::new(),
        }],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
fn sagitta_test_snapshot_with_block(
    entry_va: u64,
    entry_logical_offset: Option<u64>,
    name: &str,
    block_end_va: u64,
) -> super::analysis_state::SagittaSnapshot {
    sagitta_test_snapshot_with_blocks(
        entry_va,
        entry_logical_offset,
        name,
        vec![(entry_va, block_end_va, entry_logical_offset)],
    )
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
fn sagitta_test_snapshot_with_blocks(
    entry_va: u64,
    entry_logical_offset: Option<u64>,
    name: &str,
    block_ranges: Vec<(u64, u64, Option<u64>)>,
) -> super::analysis_state::SagittaSnapshot {
    use super::analysis_state::{
        RecoveredBlock, RecoveredConfidence, RecoveredFunction, SagittaSnapshot, SagittaSummary,
    };
    let block_starts = block_ranges
        .iter()
        .map(|(start_va, _, _)| *start_va)
        .collect::<Vec<_>>();
    let blocks = block_ranges
        .into_iter()
        .map(|(start_va, end_va, logical_offset)| RecoveredBlock {
            start_va,
            end_va,
            logical_offset,
            size: end_va.saturating_sub(start_va) as u32,
        })
        .collect::<Vec<_>>();

    SagittaSnapshot::new(
        SagittaSummary {
            functions: 1,
            blocks: blocks.len(),
            cfg_edges: 0,
            call_edges: 0,
            diagnostics: 0,
        },
        vec![RecoveredFunction {
            entry_va,
            entry_logical_offset,
            name: name.to_owned(),
            name_kind: crate::app::symbol_state::SymbolNameKind::Synthetic,
            confidence: RecoveredConfidence::Heuristic,
            provenance: Vec::new(),
            blocks: block_starts,
            callers: Vec::new(),
            callees: Vec::new(),
        }],
        blocks,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

#[cfg(all(feature = "sagitta-analysis", feature = "disasm-capstone"))]
fn sagitta_large_test_snapshot_with_target(
    function_count: usize,
) -> super::analysis_state::SagittaSnapshot {
    use super::analysis_state::{
        RecoveredBlock, RecoveredConfidence, RecoveredFunction, SagittaSnapshot, SagittaSummary,
    };

    let mut functions = Vec::with_capacity(function_count + 1);
    let mut blocks = Vec::with_capacity(function_count + 1);
    for index in 0..function_count {
        let entry_va = 0x300000 + (index as u64 * 0x10);
        functions.push(RecoveredFunction {
            entry_va,
            entry_logical_offset: Some(index as u64),
            name: format!("sub_{entry_va:x}"),
            name_kind: crate::app::symbol_state::SymbolNameKind::Synthetic,
            confidence: RecoveredConfidence::Heuristic,
            provenance: Vec::new(),
            blocks: vec![entry_va],
            callers: Vec::new(),
            callees: Vec::new(),
        });
        blocks.push(RecoveredBlock {
            start_va: entry_va,
            end_va: entry_va + 1,
            logical_offset: Some(index as u64),
            size: 1,
        });
    }

    functions.push(RecoveredFunction {
        entry_va: 0x401000,
        entry_logical_offset: Some(0x100),
        name: "sub_401000".to_owned(),
        name_kind: crate::app::symbol_state::SymbolNameKind::Synthetic,
        confidence: RecoveredConfidence::Heuristic,
        provenance: Vec::new(),
        blocks: vec![0x401000],
        callers: Vec::new(),
        callees: Vec::new(),
    });
    blocks.push(RecoveredBlock {
        start_va: 0x401000,
        end_va: 0x401003,
        logical_offset: Some(0x100),
        size: 3,
    });

    SagittaSnapshot::new(
        SagittaSummary {
            functions: functions.len(),
            blocks: blocks.len(),
            cfg_edges: 0,
            call_edges: 0,
            diagnostics: 0,
        },
        functions,
        blocks,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

#[cfg(feature = "sagitta-analysis")]
fn native_test_symbol_state(name: &str) -> crate::app::SymbolState {
    crate::app::SymbolState::from_entries(
        vec![crate::app::symbol_state::SymbolPanelEntry {
            address: 0x401000,
            name: name.to_owned(),
            name_kind: crate::app::symbol_state::SymbolNameKind::Real,
            size: 0,
            symbol_type: crate::executable::SymbolType::Function,
            source: crate::app::symbol_state::SymbolPanelEntrySource::Object,
            logical_offset: None,
            file_offset: Some(0),
            confidence_label: None,
        }],
        crate::app::symbol_state::SymbolPanelSource::Native,
    )
}

#[test]
fn side_panel_visible_rows_use_actual_panel_body_height() {
    let mut app = app_with_len(16);
    app.view_rows = 9;
    app.last_columns = Some(crate::view::layout::MainColumns {
        main_pane_kind: crate::view::layout::MainPaneKind::Hex,
        gutter: ratatui::layout::Rect::new(0, 0, 8, 10),
        sep1: ratatui::layout::Rect::new(8, 0, 1, 10),
        hex: ratatui::layout::Rect::new(9, 0, 48, 10),
        sep2: ratatui::layout::Rect::new(57, 0, 1, 10),
        ascii: ratatui::layout::Rect::new(58, 0, 16, 10),
        side_panel_sep: Some(ratatui::layout::Rect::new(74, 0, 1, 10)),
        side_panel: Some(ratatui::layout::Rect::new(75, 0, 40, 10)),
    });

    assert_eq!(app.side_panel_visible_rows(), 9);
}

#[cfg(any(feature = "disasm-capstone", feature = "sagitta-analysis"))]
fn build_disassembly_elf64(code: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0_u8; (0x100 + code.len()).max(0x200)];
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

#[cfg(all(feature = "disasm-capstone", feature = "symbols"))]
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

#[test]
fn search_wrap_disabled_does_not_wrap() {
    // Forward: cursor past the only match, wrap off -> not found, cursor stays.
    let mut app = app_with_bytes(b"hello world");
    app.config.search_wrap = false;
    app.cursor = app.document.len() - 1;
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, app.document.len() - 1);
    assert!(app.status_message.contains("not found"));
    assert!(app.status_message.contains("wrap off"));

    // Backward: cursor before the only later match, wrap off -> not found.
    let mut app2 = app_with_bytes(b"hello world hello");
    app2.config.search_wrap = false;
    app2.cursor = 0;
    app2.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();
    assert_eq!(app2.cursor, 0);
    assert!(app2.status_message.contains("wrap off"));
}

#[test]
fn unified_search_command_executes_and_warns_for_deprecated_alias() {
    let mut app = app_with_bytes(b"abc hello xyz");
    app.execute_command(crate::commands::parser::parse_command("s /hello/").unwrap())
        .unwrap();
    assert_eq!(app.cursor, 4);
    assert!(app.status_message.contains("found ascii"));

    let mut app2 = app_with_bytes(&[0x00, 0x48, 0x89, 0xc7, 0xff]);
    app2.execute_command(crate::commands::parser::parse_command("s x/48 89 c7/").unwrap())
        .unwrap();
    assert_eq!(app2.cursor, 1);
    assert!(app2.status_message.contains("found hex"));

    let mut app3 = app_with_bytes(&[0x00, 0x48, 0x89, 0xc7, 0xff]);
    app3.execute_command(crate::commands::parser::parse_command("S 48 89 c7").unwrap())
        .unwrap();
    assert_eq!(app3.cursor, 1);
    assert_eq!(app3.status_level, StatusLevel::Warning);
    assert!(app3.status_message.contains("deprecated :S"));
    assert!(app3.status_message.contains(":s x/.../"));
}

#[cfg(all(feature = "disasm-capstone", feature = "symbols"))]
#[test]
fn search_repeat_works_while_side_panel_has_focus() {
    let bytes = build_disassembly_elf64_with_symbol(
        &[
            0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xE8, 0xF5, 0xFF, 0xFF, 0xFF, 0xC3,
        ],
        "entry",
    );
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.execute_command(Command::Symbols).unwrap();
    assert_eq!(app.mode, Mode::SidePanel);

    app.execute_command(Command::SearchSymbol {
        pattern: "entry".to_owned(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 0x101);

    app.handle_action(Action::SearchNext);
    assert_eq!(app.cursor, 0x106);

    app.handle_action(Action::SearchPrev);
    assert_eq!(app.cursor, 0x101);
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
fn paste_overwrite_existing_replacement_keeps_per_byte_undo() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    app.document.replace_display_byte(1, 0xab).unwrap();
    app.cursor = 0;

    assert_eq!(app.apply_paste_overwrite(&[0xff, 0xee, 0xdd]).unwrap(), 3);

    let step = app.undo_stack.last().expect("paste should push undo");
    assert!(matches!(&step.ops[0], EditOp::ReplaceBytes { .. }));
    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
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
fn fill_existing_replacement_keeps_per_byte_undo() {
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
    assert!(matches!(&step.ops[0], EditOp::ReplaceBytes { .. }));
    app.undo(1, true).unwrap();
    assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));
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
        Some(super::analysis_state::SagittaStatus::Running)
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
        Some(super::analysis_state::SagittaStatus::Failed(message))
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Running,
        validity: super::analysis_state::AnalysisValidity::Current,
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
        revision: app.document_revision,
        snapshot: Some(sagitta_test_snapshot_for(0, Some(0x100), "sub_0")),
    });

    app.apply_paste_overwrite(&[0xff]).unwrap();
    assert_eq!(
        app.analysis_state.as_ref().unwrap().validity,
        super::analysis_state::AnalysisValidity::OutdatedBytes
    );

    let mut bulk = app_with_bytes(&[0, 1, 2, 3]);
    bulk.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
        super::analysis_state::AnalysisValidity::OutdatedBytes
    );

    app.apply_paste_insert(&[0xee]).unwrap();
    assert_eq!(
        app.analysis_state.as_ref().unwrap().validity,
        super::analysis_state::AnalysisValidity::InvalidLayout
    );
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_invalidates_tombstone_real_delete_and_resize_replace() {
    let mut tombstone = app_with_bytes(&[0, 1, 2, 3]);
    tombstone.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
        revision: tombstone.document_revision,
        snapshot: Some(sagitta_test_snapshot()),
    });
    tombstone.delete_current().unwrap();
    assert_eq!(
        tombstone.analysis_state.as_ref().unwrap().validity,
        super::analysis_state::AnalysisValidity::InvalidLayout
    );

    let mut real_delete = app_with_bytes(&[0, 1, 2, 3]);
    real_delete.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
        revision: real_delete.document_revision,
        snapshot: Some(sagitta_test_snapshot()),
    });
    real_delete.mode = Mode::InsertHex { pending: None };
    real_delete.cursor = 1;
    real_delete.edit_backspace().unwrap();
    assert_eq!(
        real_delete.analysis_state.as_ref().unwrap().validity,
        super::analysis_state::AnalysisValidity::InvalidLayout
    );

    let mut resize = app_with_bytes(&[0, 1, 2, 3]);
    resize.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
        super::analysis_state::AnalysisValidity::InvalidLayout
    );
}

#[cfg(feature = "sagitta-analysis")]
#[test]
fn sagitta_symbol_jump_outdated_allowed_invalid_layout_rejected() {
    let mut app = app_with_bytes(&[0, 1, 2, 3]);
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
    invalid.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::Current,
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::OutdatedBytes,
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
    app.analysis_state = Some(super::analysis_state::SagittaAnalysisState {
        status: super::analysis_state::SagittaStatus::Ready,
        validity: super::analysis_state::AnalysisValidity::InvalidLayout,
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
        super::memory_state::MemoryRuntime {
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
        super::memory_state::MemoryRuntime {
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
        super::memory_state::MemoryRuntime {
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
        super::memory_state::MemoryRuntime {
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
        super::memory_state::MemoryRuntime {
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
        super::memory_state::RegionEditState {
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
        super::memory_state::RegionEditState {
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
        super::memory_state::MemoryRuntime {
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
        super::memory_state::MemoryPanelView::ProcessList
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
        super::memory_state::MemoryPanelView::Maps
    );

    // Each region now occupies two body rows; region 1 starts after the header
    // rows plus region 0's two-line entry.
    let row = super::memory_state::MEMORY_MAPS_HEADER_ROWS + 2;
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
    let expected_scroll = super::memory_state::MEMORY_MAPS_HEADER_ROWS
        + super::memory_state::MEMORY_MAPS_REGION_ROWS * 2
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
        super::memory_state::MemoryPanelView::Info
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

#[cfg(feature = "disasm-capstone")]
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
