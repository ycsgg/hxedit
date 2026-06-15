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

mod app_core;
mod diff;
mod disasm;
mod edit_commands;
mod hash;
#[cfg(feature = "memory")]
mod memory;
mod search_nav;
