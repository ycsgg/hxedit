use crate::core::document::Document;
use crate::disasm::backend::DisassemblerBackend;
use crate::disasm::text::{
    parse_immediate_token, tokenize_instruction_text, InstructionTextTokenKind,
};
use crate::disasm::types::{DirectBranchTarget, DisasmRow, DisasmRowKind};
use crate::error::HxResult;
use crate::executable::{CodeSpan, ExecutableInfo};

const DATA_ROW_BYTES: usize = 8;

pub fn decode_region_rows(
    doc: &mut Document,
    info: &ExecutableInfo,
    backend: &dyn DisassemblerBackend,
    start: u64,
    max_rows: usize,
) -> HxResult<Vec<DisasmRow>> {
    if max_rows == 0 || doc.is_empty() {
        return Ok(Vec::new());
    }

    let mut rows = Vec::with_capacity(max_rows);
    let mut offset = start.min(doc.len().saturating_sub(1));

    while rows.len() < max_rows && offset < doc.len() {
        let Some(span) = current_span(info, doc, offset) else {
            break;
        };

        let row = if span.executable {
            decode_instruction_row(doc, info, backend, offset, &span)?
        } else {
            decode_data_row(doc, info, offset, &span)?
        };

        offset = offset.saturating_add(row.len() as u64);
        rows.push(row);
    }

    Ok(rows)
}

fn current_span(info: &ExecutableInfo, doc: &Document, offset: u64) -> Option<CodeSpan> {
    info.span_containing(offset)
        .cloned()
        .map(|mut span| {
            span.end_inclusive = span.end_inclusive.min(doc.len().saturating_sub(1));
            span
        })
        .or_else(|| {
            info.code_spans
                .iter()
                .find(|span| span.start >= offset)
                .cloned()
                .map(|mut span| {
                    span.start = offset;
                    span.end_inclusive = span.end_inclusive.min(doc.len().saturating_sub(1)).min(
                        span.start
                            .saturating_add(DATA_ROW_BYTES as u64)
                            .saturating_sub(1),
                    );
                    span.executable = false;
                    span.name = Some("<raw>".to_owned());
                    span
                })
        })
        .or_else(|| {
            (offset < doc.len()).then(|| CodeSpan {
                start: offset,
                end_inclusive: doc.len().saturating_sub(1).min(
                    offset
                        .saturating_add(DATA_ROW_BYTES as u64)
                        .saturating_sub(1),
                ),
                virtual_start: None,
                virtual_end_inclusive: None,
                name: Some("<raw>".to_owned()),
                executable: false,
            })
        })
}

fn decode_instruction_row(
    doc: &mut Document,
    info: &ExecutableInfo,
    backend: &dyn DisassemblerBackend,
    offset: u64,
    span: &CodeSpan,
) -> HxResult<DisasmRow> {
    let remaining = (span.end_inclusive - offset + 1) as usize;
    let read_len = backend.max_instruction_bytes().min(remaining);
    let bytes = doc.read_logical_range(offset, read_len)?;
    let virtual_address = span.virtual_address_for_offset(offset);
    let symbol_label = virtual_address
        .and_then(|address| info.display_name_at_virtual(address))
        .map(str::to_owned);
    let decode_address = virtual_address.unwrap_or(offset);
    if bytes.is_empty() {
        return Ok(DisasmRow::data(
            offset,
            virtual_address,
            Vec::new(),
            symbol_label,
            span.name.clone(),
        ));
    }

    let row = if let Some(decoded) = backend.decode_one(decode_address, &bytes)? {
        if decoded.bytes.is_empty() {
            DisasmRow::invalid(
                offset,
                virtual_address,
                bytes[0],
                symbol_label,
                span.name.clone(),
            )
        } else {
            let (text, symbolized_names) = symbolize_instruction_text(&decoded.text, info);
            let direct_target = resolve_direct_target(decoded.direct_target, info);
            DisasmRow {
                offset,
                virtual_address,
                bytes: decoded.bytes,
                text,
                symbolized_names,
                symbol_label,
                direct_target,
                span_name: span.name.clone(),
                kind: DisasmRowKind::Instruction,
            }
        }
    } else {
        DisasmRow::invalid(
            offset,
            virtual_address,
            bytes[0],
            symbol_label,
            span.name.clone(),
        )
    };
    Ok(row)
}

fn resolve_direct_target(
    direct_target: Option<DirectBranchTarget>,
    info: &ExecutableInfo,
) -> Option<DirectBranchTarget> {
    direct_target.map(|mut target| {
        target.display_name = info
            .display_name_at_virtual(target.virtual_address)
            .map(str::to_owned);
        target
    })
}

fn decode_data_row(
    doc: &mut Document,
    info: &ExecutableInfo,
    offset: u64,
    span: &CodeSpan,
) -> HxResult<DisasmRow> {
    let remaining = (span.end_inclusive - offset + 1) as usize;
    let read_len = DATA_ROW_BYTES.min(remaining);
    let bytes = doc.read_logical_range(offset, read_len)?;
    let virtual_address = span.virtual_address_for_offset(offset);
    let symbol_label = virtual_address
        .and_then(|address| info.display_name_at_virtual(address))
        .map(str::to_owned);
    Ok(DisasmRow::data(
        offset,
        virtual_address,
        bytes,
        symbol_label,
        span.name.clone(),
    ))
}

fn symbolize_instruction_text(text: &str, info: &ExecutableInfo) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut symbolized_names = Vec::new();
    for token in tokenize_instruction_text(text) {
        if token.kind != InstructionTextTokenKind::Atom {
            out.push_str(token.text);
            continue;
        }

        if let Some(address) = parse_immediate_token(token.text) {
            if let Some(name) = info.display_name_at_virtual(address) {
                if !symbolized_names.iter().any(|existing| existing == name) {
                    symbolized_names.push(name.to_owned());
                }
                out.push_str(name);
            } else {
                out.push_str(token.text);
            }
        } else {
            out.push_str(token.text);
        }
    }
    (out, symbolized_names)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::decode_region_rows;
    use crate::cli::Cli;
    use crate::core::document::Document;
    use crate::disasm::backend::{resolve_backend, resolve_backend_kind, BackendKind};
    use crate::disasm::{DirectBranchKind, DisasmRowKind};
    use crate::executable::detect_executable_info;
    use crate::executable::types::{SymbolInfo, SymbolSource, SymbolType};

    fn doc_with_bytes(bytes: &[u8]) -> Document {
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
        Document::open(&cli.file, &cli.config().unwrap()).unwrap()
    }

    fn x86_64_elf(code: &[u8]) -> Vec<u8> {
        let text_virtual = 0x401000u64;
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
        bytes[ph + 16..ph + 24].copy_from_slice(&text_virtual.to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
        bytes[0x100..0x100 + code.len()].copy_from_slice(code);
        bytes
    }

    fn aarch64_elf(code: &[u8]) -> Vec<u8> {
        let text_virtual = 0x401000u64;
        let mut bytes = vec![0_u8; 0x200];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&183u16.to_le_bytes());
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
        bytes[ph + 16..ph + 24].copy_from_slice(&text_virtual.to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
        bytes[0x100..0x100 + code.len()].copy_from_slice(code);
        bytes
    }

    fn x86_64_elf_with_plt_import_call(import_name: &str) -> Vec<u8> {
        const TEXT_OFFSET: usize = 0x100;
        const PLT_OFFSET: usize = 0x120;
        const DYNSTR_OFFSET: usize = 0x140;
        const DYNSYM_OFFSET: usize = 0x150;
        const RELA_PLT_OFFSET: usize = 0x180;
        const SHSTRTAB_OFFSET: usize = 0x1a0;
        const SHOFF: usize = 0x200;
        const SHDR_SIZE: usize = 64;
        const SECTION_COUNT: usize = 7;
        const TEXT_VIRTUAL: u64 = 0x401000;
        const PLT_VIRTUAL: u64 = 0x401020;
        const PLT_ENTRY_VIRTUAL: u64 = PLT_VIRTUAL + 0x10;

        let rel = i32::try_from(PLT_ENTRY_VIRTUAL as i64 - (TEXT_VIRTUAL as i64 + 5)).unwrap();
        let mut code = vec![0xE8];
        code.extend_from_slice(&rel.to_le_bytes());
        code.push(0xC3);

        let plt = vec![0xCC; 0x20];
        let mut dynstr = vec![0_u8];
        let import_name_offset = dynstr.len() as u32;
        dynstr.extend_from_slice(import_name.as_bytes());
        dynstr.push(0);

        let mut dynsym = vec![0_u8; 48];
        let symbol = 24usize;
        dynsym[symbol..symbol + 4].copy_from_slice(&import_name_offset.to_le_bytes());
        dynsym[symbol + 4] = 0x12;

        let mut rela_plt = Vec::with_capacity(24);
        rela_plt.extend_from_slice(&0x404000u64.to_le_bytes());
        rela_plt.extend_from_slice(&(((1_u64) << 32) | 7).to_le_bytes());
        rela_plt.extend_from_slice(&0_i64.to_le_bytes());

        let mut shstrtab = vec![0_u8];
        let text_name = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".text\0");
        let plt_name = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".plt\0");
        let dynstr_name = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".dynstr\0");
        let dynsym_name = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".dynsym\0");
        let rela_plt_name = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".rela.plt\0");
        let shstrtab_name = shstrtab.len() as u32;
        shstrtab.extend_from_slice(b".shstrtab\0");

        let mut bytes = vec![0_u8; SHOFF + SECTION_COUNT * SHDR_SIZE];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&TEXT_VIRTUAL.to_le_bytes());
        bytes[40..48].copy_from_slice(&(SHOFF as u64).to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&(SECTION_COUNT as u16).to_le_bytes());
        bytes[62..64].copy_from_slice(&6u16.to_le_bytes());

        bytes[TEXT_OFFSET..TEXT_OFFSET + code.len()].copy_from_slice(&code);
        bytes[PLT_OFFSET..PLT_OFFSET + plt.len()].copy_from_slice(&plt);
        bytes[DYNSTR_OFFSET..DYNSTR_OFFSET + dynstr.len()].copy_from_slice(&dynstr);
        bytes[DYNSYM_OFFSET..DYNSYM_OFFSET + dynsym.len()].copy_from_slice(&dynsym);
        bytes[RELA_PLT_OFFSET..RELA_PLT_OFFSET + rela_plt.len()].copy_from_slice(&rela_plt);
        bytes[SHSTRTAB_OFFSET..SHSTRTAB_OFFSET + shstrtab.len()].copy_from_slice(&shstrtab);

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

        let shdrs = &mut bytes[SHOFF..SHOFF + SECTION_COUNT * SHDR_SIZE];
        write_shdr(
            shdrs,
            ShdrSpec {
                index: 1,
                name: text_name,
                sh_type: 1,
                flags: 0x6,
                addr: TEXT_VIRTUAL,
                offset: TEXT_OFFSET as u64,
                size: code.len() as u64,
                link: 0,
                info: 0,
                addralign: 16,
                entsize: 0,
            },
        );
        write_shdr(
            shdrs,
            ShdrSpec {
                index: 2,
                name: plt_name,
                sh_type: 1,
                flags: 0x6,
                addr: PLT_VIRTUAL,
                offset: PLT_OFFSET as u64,
                size: plt.len() as u64,
                link: 0,
                info: 0,
                addralign: 16,
                entsize: 0,
            },
        );
        write_shdr(
            shdrs,
            ShdrSpec {
                index: 3,
                name: dynstr_name,
                sh_type: 3,
                flags: 0x2,
                addr: 0,
                offset: DYNSTR_OFFSET as u64,
                size: dynstr.len() as u64,
                link: 0,
                info: 0,
                addralign: 1,
                entsize: 0,
            },
        );
        write_shdr(
            shdrs,
            ShdrSpec {
                index: 4,
                name: dynsym_name,
                sh_type: 11,
                flags: 0x2,
                addr: 0,
                offset: DYNSYM_OFFSET as u64,
                size: dynsym.len() as u64,
                link: 3,
                info: 1,
                addralign: 8,
                entsize: 24,
            },
        );
        write_shdr(
            shdrs,
            ShdrSpec {
                index: 5,
                name: rela_plt_name,
                sh_type: 4,
                flags: 0x2,
                addr: 0,
                offset: RELA_PLT_OFFSET as u64,
                size: rela_plt.len() as u64,
                link: 4,
                info: 0,
                addralign: 8,
                entsize: 24,
            },
        );
        write_shdr(
            shdrs,
            ShdrSpec {
                index: 6,
                name: shstrtab_name,
                sh_type: 3,
                flags: 0,
                addr: 0,
                offset: SHSTRTAB_OFFSET as u64,
                size: shstrtab.len() as u64,
                link: 0,
                info: 0,
                addralign: 1,
                entsize: 0,
            },
        );

        bytes
    }

    fn pe64_with_text_and_rdata(text: &[u8], rdata: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x600];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        let pe = 0x80usize;
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        bytes[pe + 4..pe + 6].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[pe + 6..pe + 8].copy_from_slice(&2u16.to_le_bytes());
        bytes[pe + 20..pe + 22].copy_from_slice(&0xf0u16.to_le_bytes());
        let opt = pe + 24;
        bytes[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes[opt + 16..opt + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[opt + 60..opt + 64].copy_from_slice(&0x200u32.to_le_bytes());

        let text_sec = opt + 0xf0;
        bytes[text_sec..text_sec + 5].copy_from_slice(b".text");
        bytes[text_sec + 8..text_sec + 12].copy_from_slice(&(text.len() as u32).to_le_bytes());
        bytes[text_sec + 12..text_sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[text_sec + 16..text_sec + 20].copy_from_slice(&(text.len() as u32).to_le_bytes());
        bytes[text_sec + 20..text_sec + 24].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[text_sec + 36..text_sec + 40].copy_from_slice(&0x60000020u32.to_le_bytes());

        let rdata_sec = text_sec + 40;
        bytes[rdata_sec..rdata_sec + 6].copy_from_slice(b".rdata");
        bytes[rdata_sec + 8..rdata_sec + 12].copy_from_slice(&(rdata.len() as u32).to_le_bytes());
        bytes[rdata_sec + 12..rdata_sec + 16].copy_from_slice(&0x2000u32.to_le_bytes());
        bytes[rdata_sec + 16..rdata_sec + 20].copy_from_slice(&(rdata.len() as u32).to_le_bytes());
        bytes[rdata_sec + 20..rdata_sec + 24].copy_from_slice(&0x300u32.to_le_bytes());
        bytes[rdata_sec + 36..rdata_sec + 40].copy_from_slice(&0x40000040u32.to_le_bytes());

        bytes[0x200..0x200 + text.len()].copy_from_slice(text);
        bytes[0x300..0x300 + rdata.len()].copy_from_slice(rdata);
        bytes
    }

    #[test]
    fn resolve_backend_kind_uses_capstone_for_supported_arch() {
        let mut doc = doc_with_bytes(&x86_64_elf(&[0x90, 0xc3]));
        let info = detect_executable_info(&mut doc).unwrap();
        assert_eq!(
            resolve_backend_kind(&info, None).unwrap(),
            BackendKind::Capstone
        );
    }

    #[test]
    fn decode_region_rows_produces_x86_64_instructions() {
        let mut doc = doc_with_bytes(&x86_64_elf(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, 4).unwrap();

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].offset, 0x100);
        assert!(rows[0].text.contains("push"));
        assert!(rows[1].text.contains("mov"));
        assert!(rows[2].text.contains("nop"));
        assert!(rows[3].text.contains("ret"));
    }

    #[test]
    fn decode_region_rows_produces_aarch64_instructions() {
        let mut doc = doc_with_bytes(&aarch64_elf(&[
            0x20, 0x00, 0x80, 0xd2, 0xc0, 0x03, 0x5f, 0xd6,
        ]));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, 2).unwrap();

        assert_eq!(rows.len(), 2);
        assert!(rows[0].text.contains("mov") || rows[0].text.contains("orr"));
        assert!(rows[1].text.contains("ret"));
    }

    #[test]
    fn decode_region_rows_falls_back_to_db_for_invalid_bytes() {
        let mut doc = doc_with_bytes(&x86_64_elf(&[0x0f, 0xc3]));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, 2).unwrap();

        assert_eq!(rows[0].text, ".db 0x0f");
        assert!(rows[1].text.contains("ret"));
    }

    #[test]
    fn decode_region_rows_emits_data_rows_for_non_executable_spans() {
        let mut doc = doc_with_bytes(&pe64_with_text_and_rdata(&[0x90, 0xc3], b"ABC"));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x300, 1).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, DisasmRowKind::Data);
        assert_eq!(rows[0].span_name.as_deref(), Some(".rdata"));
        assert_eq!(rows[0].text, ".db 0x41, 0x42, 0x43");
    }

    #[test]
    fn decode_region_rows_tracks_x86_direct_call_targets() {
        let mut doc = doc_with_bytes(&x86_64_elf(&[0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xC3]));
        let mut info = detect_executable_info(&mut doc).unwrap();
        info.symbols_by_va.insert(
            0x401000,
            SymbolInfo {
                display_name: "entry".to_owned(),
                raw_name: Some("entry".to_owned()),
                source: SymbolSource::Object,
                size: 0,
                symbol_type: SymbolType::Function,
            },
        );
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, 3).unwrap();

        assert_eq!(rows[1].text, "call entry");
        assert_eq!(rows[1].symbolized_names, vec!["entry".to_owned()]);
        let target = rows[1].direct_target.as_ref().expect("direct target");
        assert_eq!(target.kind, DirectBranchKind::Call);
        assert_eq!(target.virtual_address, 0x401000);
        assert_eq!(target.display_name.as_deref(), Some("entry"));
    }

    #[test]
    fn decode_region_rows_tracks_aarch64_direct_call_targets() {
        let mut doc = doc_with_bytes(&aarch64_elf(&[
            0x00, 0x00, 0x00, 0x94, 0xc0, 0x03, 0x5f, 0xd6,
        ]));
        let mut info = detect_executable_info(&mut doc).unwrap();
        info.symbols_by_va.insert(
            0x401000,
            SymbolInfo {
                display_name: "entry".to_owned(),
                raw_name: Some("entry".to_owned()),
                source: SymbolSource::Object,
                size: 0,
                symbol_type: SymbolType::Function,
            },
        );
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, 2).unwrap();

        assert_eq!(rows[0].text, "bl entry");
        assert_eq!(rows[0].symbolized_names, vec!["entry".to_owned()]);
        let target = rows[0].direct_target.as_ref().expect("direct target");
        assert_eq!(target.kind, DirectBranchKind::Call);
        assert_eq!(target.virtual_address, 0x401000);
        assert_eq!(target.display_name.as_deref(), Some("entry"));
    }

    #[test]
    fn decode_region_rows_tracks_aarch64_multi_immediate_jump_targets() {
        let mut doc = doc_with_bytes(&aarch64_elf(&[
            0x20, 0x00, 0x00, 0x37, 0xc0, 0x03, 0x5f, 0xd6,
        ]));
        let mut info = detect_executable_info(&mut doc).unwrap();
        info.symbols_by_va.insert(
            0x401004,
            SymbolInfo {
                display_name: "target".to_owned(),
                raw_name: Some("target".to_owned()),
                source: SymbolSource::Object,
                size: 0,
                symbol_type: SymbolType::Function,
            },
        );
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, 2).unwrap();

        assert_eq!(rows[0].text, "tbnz w0, #0, target");
        assert_eq!(rows[0].symbolized_names, vec!["target".to_owned()]);
        let target = rows[0].direct_target.as_ref().expect("direct target");
        assert_eq!(target.kind, DirectBranchKind::Jump);
        assert_eq!(target.virtual_address, 0x401004);
        assert_eq!(target.display_name.as_deref(), Some("target"));
    }

    #[test]
    fn decode_region_rows_resolves_elf_plt_import_targets() {
        let mut doc = doc_with_bytes(&x86_64_elf_with_plt_import_call("puts"));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let rows = decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, 2).unwrap();

        assert!(info.symbol_at_virtual(0x401030).is_none());
        assert_eq!(info.display_name_at_virtual(0x401030), Some("puts"));
        assert_eq!(rows[0].text, "call puts");
        assert_eq!(rows[0].symbolized_names, vec!["puts".to_owned()]);
        let target = rows[0].direct_target.as_ref().expect("direct target");
        assert_eq!(target.kind, DirectBranchKind::Call);
        assert_eq!(target.virtual_address, 0x401030);
        assert_eq!(target.display_name.as_deref(), Some("puts"));
    }
}
