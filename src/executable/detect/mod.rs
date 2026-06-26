mod elf;
mod macho;
mod pe;
mod util;

#[cfg(feature = "symbols")]
mod object_meta;

use crate::core::document::Document;
use crate::error::{HxError, HxResult};
use crate::executable::types::{
    Bitness, CodeSpan, Endian, ExecutableArch, ExecutableInfo, ExecutableKind,
};

pub fn detect_executable_info(doc: &mut Document) -> Option<ExecutableInfo> {
    #[cfg(feature = "symbols")]
    let mut info = elf::detect(doc)
        .or_else(|| pe::detect(doc))
        .or_else(|| macho::detect(doc))?;
    #[cfg(not(feature = "symbols"))]
    let info = elf::detect(doc)
        .or_else(|| pe::detect(doc))
        .or_else(|| macho::detect(doc))?;
    #[cfg(feature = "symbols")]
    object_meta::enrich(doc, &mut info);
    Some(info)
}

pub fn force_raw_executable_info(
    doc_len: u64,
    raw_arch: &str,
    offset: u64,
) -> HxResult<ExecutableInfo> {
    let arch =
        parse_arch(raw_arch).ok_or_else(|| HxError::UnknownDisassemblyArch(raw_arch.to_owned()))?;
    if doc_len == 0 || offset >= doc_len {
        return Err(HxError::OffsetOutOfRange);
    }
    Ok(ExecutableInfo {
        kind: ExecutableKind::Raw,
        arch,
        bitness: match arch {
            ExecutableArch::X86 | ExecutableArch::Arm => Bitness::Bit32,
            ExecutableArch::X86_64 | ExecutableArch::AArch64 | ExecutableArch::RiscV64 => {
                Bitness::Bit64
            }
            ExecutableArch::Unknown => Bitness::Bit64,
        },
        endian: Endian::Little,
        entry_offset: Some(offset),
        entry_virtual_address: Some(offset),
        code_spans: vec![CodeSpan {
            start: offset,
            end_inclusive: doc_len - 1,
            virtual_start: Some(offset),
            virtual_end_inclusive: Some(doc_len - 1),
            name: Some("<raw>".to_owned()),
            executable: true,
        }],
        symbols_by_va: Default::default(),
        target_names_by_va: Default::default(),
        symbols_by_name: Default::default(),
        target_names_by_name: Default::default(),
        imports: Vec::new(),
    })
}

pub fn override_arch(info: &ExecutableInfo, raw: &str) -> HxResult<ExecutableInfo> {
    let arch = parse_arch(raw).ok_or_else(|| HxError::UnknownDisassemblyArch(raw.to_owned()))?;
    let mut updated = info.clone();
    updated.arch = arch;
    updated.bitness = match arch {
        ExecutableArch::X86 | ExecutableArch::Arm => Bitness::Bit32,
        ExecutableArch::X86_64 | ExecutableArch::AArch64 | ExecutableArch::RiscV64 => {
            Bitness::Bit64
        }
        ExecutableArch::Unknown => updated.bitness,
    };
    Ok(updated)
}

fn parse_arch(raw: &str) -> Option<ExecutableArch> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "x86" | "i386" | "i686" => Some(ExecutableArch::X86),
        "x86_64" | "x64" | "amd64" => Some(ExecutableArch::X86_64),
        "arm" | "armv7" => Some(ExecutableArch::Arm),
        "aarch64" | "arm64" => Some(ExecutableArch::AArch64),
        "riscv64" => Some(ExecutableArch::RiscV64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use std::fs;
    use tempfile::tempdir;

    use super::*;

    fn doc_with_bytes(bytes: &[u8]) -> Document {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, bytes).unwrap();
        let cli = Cli {
            file: Some(file),
            remote: None,
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
            run: Vec::new(),
            command: Vec::new(),
            select: None,
            script: Vec::new(),
        };
        Document::open(cli.file.as_ref().unwrap(), &cli.config().unwrap()).unwrap()
    }

    fn build_elf64() -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x200];
        bytes[0..4].copy_from_slice(b"ELF");
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
    }

    #[cfg(feature = "symbols")]
    fn build_elf64_with_symbol(code: &[u8], symbol_name: &str) -> Vec<u8> {
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
        bytes[0..4].copy_from_slice(b"ELF");
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

    fn build_pe64() -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x400];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        let pe = 0x80usize;
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        bytes[pe + 4..pe + 6].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes());
        bytes[pe + 20..pe + 22].copy_from_slice(&0xf0u16.to_le_bytes());
        let opt = pe + 24;
        bytes[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes[opt + 16..opt + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[opt + 60..opt + 64].copy_from_slice(&0x200u32.to_le_bytes());
        let sec = opt + 0xf0;
        bytes[sec..sec + 5].copy_from_slice(b".text");
        bytes[sec + 8..sec + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[sec + 16..sec + 20].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[sec + 36..sec + 40].copy_from_slice(&0x60000020u32.to_le_bytes());
        bytes
    }

    fn build_macho64() -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x400];
        bytes[0..4].copy_from_slice(&0xcffaedfeu32.to_be_bytes());
        bytes[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&176u32.to_le_bytes());
        let lc_segment = 32usize;
        bytes[lc_segment..lc_segment + 4].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[lc_segment + 4..lc_segment + 8].copy_from_slice(&152u32.to_le_bytes());
        bytes[lc_segment + 8..lc_segment + 14].copy_from_slice(b"__TEXT");
        bytes[lc_segment + 24..lc_segment + 32].copy_from_slice(&0x1000u64.to_le_bytes());
        bytes[lc_segment + 32..lc_segment + 40].copy_from_slice(&0x20u64.to_le_bytes());
        bytes[lc_segment + 40..lc_segment + 48].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[lc_segment + 48..lc_segment + 56].copy_from_slice(&0x20u64.to_le_bytes());
        bytes[lc_segment + 56..lc_segment + 60].copy_from_slice(&0x7u32.to_le_bytes());
        bytes[lc_segment + 64..lc_segment + 68].copy_from_slice(&1u32.to_le_bytes());
        let sect = lc_segment + 72;
        bytes[sect..sect + 6].copy_from_slice(b"__text");
        bytes[sect + 16..sect + 22].copy_from_slice(b"__TEXT");
        bytes[sect + 32..sect + 40].copy_from_slice(&0x1000u64.to_le_bytes());
        bytes[sect + 40..sect + 48].copy_from_slice(&0x20u64.to_le_bytes());
        bytes[sect + 48..sect + 52].copy_from_slice(&0x100u32.to_le_bytes());
        bytes[sect + 64..sect + 68].copy_from_slice(&0x00000400u32.to_le_bytes());
        let lc_main = lc_segment + 152;
        bytes[lc_main..lc_main + 4].copy_from_slice(&0x80000028u32.to_le_bytes());
        bytes[lc_main + 4..lc_main + 8].copy_from_slice(&24u32.to_le_bytes());
        bytes[lc_main + 8..lc_main + 16].copy_from_slice(&0x100u64.to_le_bytes());
        bytes
    }

    #[test]
    fn detects_elf_executable_info() {
        let mut doc = doc_with_bytes(&build_elf64());
        let info = detect_executable_info(&mut doc).expect("elf");
        assert_eq!(info.kind, ExecutableKind::Elf);
        assert_eq!(info.arch, ExecutableArch::X86_64);
        assert_eq!(info.entry_offset, Some(0x100));
        assert_eq!(
            info.first_executable_span().map(|span| span.start),
            Some(0x100)
        );
    }

    #[test]
    fn detects_pe_executable_info() {
        let mut doc = doc_with_bytes(&build_pe64());
        let info = detect_executable_info(&mut doc).expect("pe");
        assert_eq!(info.kind, ExecutableKind::Pe);
        assert_eq!(info.arch, ExecutableArch::X86_64);
        assert_eq!(info.entry_offset, Some(0x200));
        assert!(info
            .code_spans
            .iter()
            .any(|span| span.name.as_deref() == Some(".text")));
        assert_eq!(
            info.first_executable_span().map(|span| span.start),
            Some(0x200)
        );
    }

    #[test]
    fn detects_macho_executable_info() {
        let mut doc = doc_with_bytes(&build_macho64());
        let info = detect_executable_info(&mut doc).expect("macho");
        assert_eq!(info.kind, ExecutableKind::MachO);
        assert_eq!(info.arch, ExecutableArch::AArch64);
        assert_eq!(info.entry_offset, Some(0x100));
        assert!(info.code_spans.iter().any(|span| span.start == 0x100));
    }

    #[cfg(feature = "symbols")]
    #[test]
    fn object_metadata_enrichment_uses_current_document_bytes() {
        let mut doc = doc_with_bytes(&build_elf64_with_symbol(&[0x90, 0xc3], "entry"));
        for (idx, byte) in b"patch".iter().copied().enumerate() {
            doc.replace_display_byte(0x121 + idx as u64, byte).unwrap();
        }

        let info = detect_executable_info(&mut doc).expect("elf");

        assert!(info.symbols_by_name.contains_key("patch"));
        assert!(!info.symbols_by_name.contains_key("entry"));
    }

    #[test]
    fn override_arch_accepts_common_aliases() {
        let info = ExecutableInfo {
            kind: ExecutableKind::Elf,
            arch: ExecutableArch::Unknown,
            bitness: Bitness::Bit64,
            endian: Endian::Little,
            entry_offset: None,
            entry_virtual_address: None,
            code_spans: Vec::new(),
            symbols_by_va: Default::default(),
            target_names_by_va: Default::default(),
            symbols_by_name: Default::default(),
            target_names_by_name: Default::default(),
            imports: Vec::new(),
        };
        let updated = override_arch(&info, "arm64").unwrap();
        assert_eq!(updated.arch, ExecutableArch::AArch64);
        assert_eq!(updated.bitness, Bitness::Bit64);
    }
}
