use crate::core::document::Document;
use crate::format::detect::{read_bytes_raw, read_u8, DEFAULT_ENTRY_CAP};
use crate::format::types::*;

const ELF_MAGIC: [u8; 4] = [0x7f, 0x45, 0x4c, 0x46];

const ELF64_EHDR_SIZE: u64 = 64;
const ELF32_EHDR_SIZE: u64 = 52;
const ELF64_PHDR_SIZE: u64 = 56;
const ELF32_PHDR_SIZE: u64 = 32;
const ELF64_SHDR_SIZE: u64 = 64;
const ELF32_SHDR_SIZE: u64 = 40;

const PT_NULL: u32 = 0;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const PT_SHLIB: u32 = 5;
const PT_PHDR: u32 = 6;
const PT_TLS: u32 = 7;
const PT_GNU_EH_FRAME: u32 = 0x6474e550;
const PT_GNU_STACK: u32 = 0x6474e551;
const PT_GNU_RELRO: u32 = 0x6474e552;
const PT_GNU_PROPERTY: u32 = 0x6474e553;

const SHT_NULL: u32 = 0;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHT_HASH: u32 = 5;
const SHT_DYNAMIC: u32 = 6;
const SHT_NOTE: u32 = 7;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;
const SHT_DYNSYM: u32 = 11;
const SHT_GNU_HASH: u32 = 0x6ffffff6;
const SHT_GNU_VERDEF: u32 = 0x6ffffffd;
const SHT_GNU_VERNEED: u32 = 0x6ffffffe;
const SHT_GNU_VERSYM: u32 = 0x6fffffff;

const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_PLTRELSZ: u64 = 2;
const DT_PLTGOT: u64 = 3;
const DT_HASH: u64 = 4;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_RELA: u64 = 7;
const DT_RELASZ: u64 = 8;
const DT_RELAENT: u64 = 9;
const DT_STRSZ: u64 = 10;
const DT_SYMENT: u64 = 11;
const DT_INIT: u64 = 12;
const DT_FINI: u64 = 13;
const DT_SONAME: u64 = 14;
const DT_RPATH: u64 = 15;
const DT_SYMBOLIC: u64 = 16;
const DT_REL: u64 = 17;
const DT_RELSZ: u64 = 18;
const DT_RELENT: u64 = 19;
const DT_PLTREL: u64 = 20;
const DT_DEBUG: u64 = 21;
const DT_TEXTREL: u64 = 22;
const DT_JMPREL: u64 = 23;
const DT_BIND_NOW: u64 = 24;
const DT_INIT_ARRAY: u64 = 25;
const DT_FINI_ARRAY: u64 = 26;
const DT_INIT_ARRAYSZ: u64 = 27;
const DT_FINI_ARRAYSZ: u64 = 28;
const DT_RUNPATH: u64 = 29;
const DT_FLAGS: u64 = 30;
const DT_PREINIT_ARRAY: u64 = 32;
const DT_PREINIT_ARRAYSZ: u64 = 33;
const DT_SYMTAB_SHNDX: u64 = 34;
const DT_GNU_HASH: u64 = 0x6ffffef5;
const DT_FLAGS_1: u64 = 0x6ffffffb;
const DT_VERDEF: u64 = 0x6ffffffc;
const DT_VERDEFNUM: u64 = 0x6ffffffd;
const DT_VERNEED: u64 = 0x6ffffffe;
const DT_VERNEEDNUM: u64 = 0x6fffffff;

const NT_GNU_BUILD_ID: u32 = 3;
const NT_GNU_PROPERTY_TYPE_0: u32 = 5;

const GNU_PROPERTY_STACK_SIZE: u32 = 1;
const GNU_PROPERTY_NO_COPY_ON_PROTECTED: u32 = 2;
const GNU_PROPERTY_X86_FEATURE_1_AND: u32 = 0xc0000002;
const GNU_PROPERTY_X86_ISA_1_USED: u32 = 0xc0010002;
const GNU_PROPERTY_X86_ISA_1_NEEDED: u32 = 0xc0008002;
const GNU_PROPERTY_AARCH64_FEATURE_1_AND: u32 = 0xc0000000;

#[derive(Debug)]
struct ProgramHeaderInfo {
    index: usize,
    entry_offset: u64,
    p_type: u32,
    offset: u64,
    filesz: u64,
}

#[derive(Debug)]
struct SectionHeaderInfo {
    index: usize,
    entry_offset: u64,
    name_offset: u32,
    name: String,
    sh_type: u32,
    offset: u64,
    size: u64,
    link: u32,
    entsize: u64,
}

#[derive(Debug)]
struct SymbolInfo {
    entry_offset: u64,
    name_offset: u32,
    name: String,
    info: u8,
    other: u8,
}

struct HeaderSummary {
    fields: Vec<FieldDef>,
    phoff: u64,
    phnum: usize,
    phentsize: u64,
    shoff: u64,
    shnum: usize,
    shentsize: u64,
    shstrndx: usize,
}

mod header;
mod layout;
mod payloads;
mod structures;
mod symbols;
mod versions;

use layout::*;

#[cfg(test)]
mod tests;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < 16 {
        return None;
    }

    let magic = read_bytes_raw(doc, 0, 4)?;
    if magic != ELF_MAGIC {
        return None;
    }

    let ei_class = read_u8(doc, 4)?;
    let ei_data = read_u8(doc, 5)?;
    let is_64 = ei_class == 2;
    let is_le = ei_data == 1;
    if !matches!(ei_class, 1 | 2) || !matches!(ei_data, 1 | 2) {
        return None;
    }

    ElfParser::new(doc, is_64, is_le, entry_cap).parse()
}

struct ElfParser<'a> {
    doc: &'a mut Document,
    is_64: bool,
    is_le: bool,
    entry_cap: usize,
}

impl<'a> ElfParser<'a> {
    fn new(doc: &'a mut Document, is_64: bool, is_le: bool, entry_cap: usize) -> Self {
        Self {
            doc,
            is_64,
            is_le,
            entry_cap,
        }
    }

    fn parse(&mut self) -> Option<FormatDef> {
        if self.doc.len()
            < if self.is_64 {
                ELF64_EHDR_SIZE
            } else {
                ELF32_EHDR_SIZE
            }
        {
            return None;
        }

        let header = self.build_header_fields()?;

        let program_headers =
            self.parse_program_headers(header.phoff, header.phnum, header.phentsize);
        let section_headers = self.parse_section_headers(
            header.shoff,
            header.shnum,
            header.shentsize,
            header.shstrndx,
        );

        let mut children = Vec::new();
        if !program_headers.is_empty() {
            children.push(self.build_program_header_table(
                header.phoff,
                header.phnum,
                &program_headers,
                &section_headers,
            ));
        }
        if !section_headers.is_empty() {
            children.push(self.build_section_header_table(
                header.shoff,
                header.shnum,
                &section_headers,
            ));
        }

        Some(FormatDef {
            name: if self.is_64 { "ELF64" } else { "ELF32" }.to_string(),
            structs: vec![StructDef {
                name: if self.is_64 {
                    "ELF64 Header"
                } else {
                    "ELF32 Header"
                }
                .to_string(),
                base_offset: 0,
                fields: header.fields,
                children,
            }],
        })
    }

    fn shown_count(&self, total: usize) -> usize {
        total.min(self.entry_cap.max(1))
    }

    fn header_layout(&self) -> HeaderLayout {
        header_layout(self.is_64)
    }

    fn program_header_layout(&self) -> ProgramHeaderLayout {
        program_header_layout(self.is_64)
    }

    fn section_header_layout(&self) -> SectionHeaderLayout {
        section_header_layout(self.is_64)
    }

    fn symbol_layout(&self) -> SymbolLayout {
        symbol_layout(self.is_64)
    }

    fn u16_t(&self) -> FieldType {
        if self.is_le {
            FieldType::U16Le
        } else {
            FieldType::U16Be
        }
    }

    fn u32_t(&self) -> FieldType {
        if self.is_le {
            FieldType::U32Le
        } else {
            FieldType::U32Be
        }
    }

    fn u64_t(&self) -> FieldType {
        if self.is_le {
            FieldType::U64Le
        } else {
            FieldType::U64Be
        }
    }

    fn word_t(&self) -> FieldType {
        if self.is_64 {
            self.u64_t()
        } else {
            self.u32_t()
        }
    }

    fn sword_t(&self) -> FieldType {
        if self.is_64 {
            if self.is_le {
                FieldType::I64Le
            } else {
                FieldType::I64Be
            }
        } else if self.is_le {
            FieldType::I32Le
        } else {
            FieldType::I32Be
        }
    }

    fn read_word(&mut self, offset: u64) -> Option<u64> {
        if self.is_64 {
            self.read_u64(offset)
        } else {
            self.read_u32(offset).map(u64::from)
        }
    }

    fn require_bytes(&mut self, offset: u64, len: u64) -> Option<()> {
        let len = usize::try_from(len).ok()?;
        read_bytes_raw(self.doc, offset, len).map(|_| ())
    }

    fn read_u16(&mut self, offset: u64) -> Option<u16> {
        let b = read_bytes_raw(self.doc, offset, 2)?;
        Some(if self.is_le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    }

    fn read_u32(&mut self, offset: u64) -> Option<u32> {
        let b = read_bytes_raw(self.doc, offset, 4)?;
        Some(if self.is_le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    }

    fn read_u64(&mut self, offset: u64) -> Option<u64> {
        let b = read_bytes_raw(self.doc, offset, 8)?;
        Some(if self.is_le {
            u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        } else {
            u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        })
    }
}
