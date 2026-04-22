use crate::core::document::Document;
use crate::disasm::backend::DisassemblerBackend;
use crate::disasm::text::{
    parse_immediate_token, tokenize_instruction_text, InstructionTextTokenKind,
};
use crate::disasm::types::DisasmRow;
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
        .and_then(|address| info.symbol_at_virtual(address))
        .map(|symbol| symbol.display_name.clone());
    if bytes.is_empty() {
        return Ok(DisasmRow::data(
            offset,
            virtual_address,
            Vec::new(),
            symbol_label,
            span.name.clone(),
        ));
    }

    let row = if let Some(decoded) = backend.decode_one(offset, &bytes)? {
        if decoded.bytes.is_empty() {
            DisasmRow::invalid(
                offset,
                virtual_address,
                bytes[0],
                symbol_label,
                span.name.clone(),
            )
        } else {
            let text = symbolize_instruction_text(&decoded.text, info);
            DisasmRow::instruction(
                offset,
                virtual_address,
                decoded.bytes,
                text,
                symbol_label,
                span.name.clone(),
            )
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
        .and_then(|address| info.symbol_at_virtual(address))
        .map(|symbol| symbol.display_name.clone());
    Ok(DisasmRow::data(
        offset,
        virtual_address,
        bytes,
        symbol_label,
        span.name.clone(),
    ))
}

fn symbolize_instruction_text(text: &str, info: &ExecutableInfo) -> String {
    let mut out = String::with_capacity(text.len());
    for token in tokenize_instruction_text(text) {
        if token.kind != InstructionTextTokenKind::Atom {
            out.push_str(token.text);
            continue;
        }

        if let Some(address) = parse_immediate_token(token.text) {
            if let Some(symbol) = info.symbol_at_virtual(address) {
                out.push_str(&symbol.display_name);
            } else {
                out.push_str(token.text);
            }
        } else {
            out.push_str(token.text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::decode_region_rows;
    use crate::cli::Cli;
    use crate::core::document::Document;
    use crate::disasm::backend::{resolve_backend, resolve_backend_kind, BackendKind};
    use crate::disasm::DisasmRowKind;
    use crate::executable::detect_executable_info;

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

    fn aarch64_elf(code: &[u8]) -> Vec<u8> {
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
        bytes[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
        bytes[0x100..0x100 + code.len()].copy_from_slice(code);
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
}
