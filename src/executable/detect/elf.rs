use crate::core::document::Document;
use crate::executable::types::{
    Bitness, CodeSpan, Endian, ExecutableArch, ExecutableInfo, ExecutableKind,
};

use super::util::{push_span, read_u16, read_u32, read_u64};

pub(super) fn detect(doc: &mut Document) -> Option<ExecutableInfo> {
    let ident = doc.read_logical_range(0, 0x40).ok()?;
    if ident.len() < 0x18 || &ident[0..4] != b"ELF" {
        return None;
    }
    let class = ident[4];
    let data = ident[5];
    let (bitness, is_64) = match class {
        1 => (Bitness::Bit32, false),
        2 => (Bitness::Bit64, true),
        _ => return None,
    };
    let endian = match data {
        1 => Endian::Little,
        2 => Endian::Big,
        _ => return None,
    };

    let machine = read_u16(&ident, 18, endian)?;
    let arch = match machine {
        3 => ExecutableArch::X86,
        0x3e => ExecutableArch::X86_64,
        40 => ExecutableArch::Arm,
        183 => ExecutableArch::AArch64,
        243 => ExecutableArch::RiscV64,
        _ => ExecutableArch::Unknown,
    };

    let entry_virtual_address = if is_64 {
        read_u64(&ident, 24, endian)
    } else {
        read_u32(&ident, 24, endian).map(u64::from)
    };
    let phoff = if is_64 {
        read_u64(&ident, 32, endian)?
    } else {
        u64::from(read_u32(&ident, 28, endian)?)
    };
    let shoff = if is_64 {
        read_u64(&ident, 40, endian)?
    } else {
        u64::from(read_u32(&ident, 32, endian)?)
    };
    let phentsize = u64::from(read_u16(&ident, if is_64 { 54 } else { 42 }, endian)?);
    let phnum = usize::from(read_u16(&ident, if is_64 { 56 } else { 44 }, endian)?);
    let shentsize = u64::from(read_u16(&ident, if is_64 { 58 } else { 46 }, endian)?);
    let shnum = usize::from(read_u16(&ident, if is_64 { 60 } else { 48 }, endian)?);
    let shstrndx = usize::from(read_u16(&ident, if is_64 { 62 } else { 50 }, endian)?);

    let mut code_spans =
        detect_section_spans(doc, endian, is_64, shoff, shentsize, shnum, shstrndx)
            .unwrap_or_default();
    if phentsize > 0 && phnum > 0 {
        let total = phentsize.checked_mul(phnum as u64)?;
        let phdrs = doc.read_logical_range(phoff, total as usize).ok()?;
        if phdrs.len() < total as usize {
            return None;
        }
        for idx in 0..phnum {
            let base = idx * phentsize as usize;
            let p_type = read_u32(&phdrs, base, endian)?;
            if p_type != 1 {
                continue;
            }
            let flags = if is_64 {
                read_u32(&phdrs, base + 4, endian)?
            } else {
                read_u32(&phdrs, base + 24, endian)?
            };
            let offset = if is_64 {
                read_u64(&phdrs, base + 8, endian)?
            } else {
                u64::from(read_u32(&phdrs, base + 4, endian)?)
            };
            let filesz = if is_64 {
                read_u64(&phdrs, base + 32, endian)?
            } else {
                u64::from(read_u32(&phdrs, base + 16, endian)?)
            };
            if filesz == 0 {
                continue;
            }
            push_span(
                &mut code_spans,
                CodeSpan {
                    start: offset,
                    end_inclusive: offset + filesz - 1,
                    virtual_start: Some(if is_64 {
                        read_u64(&phdrs, base + 16, endian)?
                    } else {
                        u64::from(read_u32(&phdrs, base + 8, endian)?)
                    }),
                    virtual_end_inclusive: Some(
                        if is_64 {
                            read_u64(&phdrs, base + 16, endian)?
                        } else {
                            u64::from(read_u32(&phdrs, base + 8, endian)?)
                        }
                        .saturating_add(filesz.saturating_sub(1)),
                    ),
                    name: Some(format!("PT_LOAD#{idx}")),
                    executable: flags & 0x1 != 0,
                },
            );
        }
    }

    Some(ExecutableInfo {
        kind: ExecutableKind::Elf,
        arch,
        bitness,
        endian,
        entry_offset: entry_virtual_address,
        entry_virtual_address,
        code_spans,
        symbols_by_va: Default::default(),
        symbols_by_name: Default::default(),
        imports: Vec::new(),
    })
}

fn detect_section_spans(
    doc: &mut Document,
    endian: Endian,
    is_64: bool,
    shoff: u64,
    shentsize: u64,
    shnum: usize,
    shstrndx: usize,
) -> Option<Vec<CodeSpan>> {
    if shoff == 0 || shentsize == 0 || shnum == 0 || shstrndx >= shnum {
        return None;
    }
    let total = shentsize.checked_mul(shnum as u64)?;
    let shdrs = doc.read_logical_range(shoff, total as usize).ok()?;
    if shdrs.len() < total as usize {
        return None;
    }

    let shstr_base = shstrndx.checked_mul(shentsize as usize)?;
    let shstr_offset = if is_64 {
        read_u64(&shdrs, shstr_base + 24, endian)?
    } else {
        u64::from(read_u32(&shdrs, shstr_base + 16, endian)?)
    };
    let shstr_size = if is_64 {
        read_u64(&shdrs, shstr_base + 32, endian)?
    } else {
        u64::from(read_u32(&shdrs, shstr_base + 20, endian)?)
    };
    let names = doc
        .read_logical_range(shstr_offset, shstr_size as usize)
        .ok()?;
    if names.len() < shstr_size as usize {
        return None;
    }

    let mut spans = Vec::new();
    for idx in 0..shnum {
        let base = idx * shentsize as usize;
        let sh_name = read_u32(&shdrs, base, endian)? as usize;
        let sh_type = read_u32(&shdrs, base + 4, endian)?;
        let sh_flags = if is_64 {
            read_u64(&shdrs, base + 8, endian)?
        } else {
            u64::from(read_u32(&shdrs, base + 8, endian)?)
        };
        let sh_addr = if is_64 {
            read_u64(&shdrs, base + 16, endian)?
        } else {
            u64::from(read_u32(&shdrs, base + 12, endian)?)
        };
        let sh_offset = if is_64 {
            read_u64(&shdrs, base + 24, endian)?
        } else {
            u64::from(read_u32(&shdrs, base + 16, endian)?)
        };
        let sh_size = if is_64 {
            read_u64(&shdrs, base + 32, endian)?
        } else {
            u64::from(read_u32(&shdrs, base + 20, endian)?)
        };
        if sh_size == 0 || sh_type == 8 {
            continue;
        }
        push_span(
            &mut spans,
            CodeSpan {
                start: sh_offset,
                end_inclusive: sh_offset + sh_size - 1,
                virtual_start: Some(sh_addr),
                virtual_end_inclusive: Some(sh_addr.saturating_add(sh_size.saturating_sub(1))),
                name: Some(
                    read_elf_string(&names, sh_name).unwrap_or_else(|| format!("section#{idx}")),
                ),
                executable: sh_flags & 0x4 != 0,
            },
        );
    }

    (!spans.is_empty()).then_some(spans)
}

fn read_elf_string(table: &[u8], offset: usize) -> Option<String> {
    let bytes = table.get(offset..)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let name = std::str::from_utf8(&bytes[..end]).ok()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}
