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

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ProgramHeaderInfo {
    index: usize,
    entry_offset: u64,
    p_type: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct SectionHeaderInfo {
    index: usize,
    entry_offset: u64,
    name_offset: u32,
    name: String,
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

        let (fields, phoff, phnum, phentsize, shoff, shnum, shentsize, shstrndx) =
            self.build_header_fields()?;

        let program_headers = self.parse_program_headers(phoff, phnum, phentsize);
        let section_headers = self.parse_section_headers(shoff, shnum, shentsize, shstrndx);

        let mut children = Vec::new();
        if !program_headers.is_empty() {
            children.push(self.build_program_header_table(
                phoff,
                phnum,
                &program_headers,
                &section_headers,
            ));
        }
        if !section_headers.is_empty() {
            children.push(self.build_section_header_table(shoff, shnum, &section_headers));
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
                fields,
                children,
            }],
        })
    }

    fn build_header_fields(
        &mut self,
    ) -> Option<(Vec<FieldDef>, u64, usize, u64, u64, usize, u64, usize)> {
        let u16_t = self.u16_t();
        let u32_t = self.u32_t();
        let u64_t = self.u64_t();
        let addr_t = if self.is_64 {
            u64_t.clone()
        } else {
            u32_t.clone()
        };
        let off_t = addr_t.clone();

        let mut fields = vec![
            FieldDef {
                name: "e_ident".into(),
                offset: 0,
                field_type: FieldType::Bytes(16),
                description: "ELF identification".into(),
                editable: false,
            },
            FieldDef {
                name: "ei_class".into(),
                offset: 4,
                field_type: FieldType::Enum {
                    inner: Box::new(FieldType::U8),
                    variants: vec![(1, "32-bit".into()), (2, "64-bit".into())],
                },
                description: "File class".into(),
                editable: true,
            },
            FieldDef {
                name: "ei_data".into(),
                offset: 5,
                field_type: FieldType::Enum {
                    inner: Box::new(FieldType::U8),
                    variants: vec![(1, "Little-endian".into()), (2, "Big-endian".into())],
                },
                description: "Data encoding".into(),
                editable: true,
            },
            FieldDef {
                name: "ei_version".into(),
                offset: 6,
                field_type: FieldType::U8,
                description: "ELF version".into(),
                editable: true,
            },
            FieldDef {
                name: "ei_osabi".into(),
                offset: 7,
                field_type: FieldType::Enum {
                    inner: Box::new(FieldType::U8),
                    variants: vec![(0, "ELFOSABI_NONE".into()), (3, "ELFOSABI_LINUX".into())],
                },
                description: "OS/ABI identification".into(),
                editable: true,
            },
            FieldDef {
                name: "e_type".into(),
                offset: 16,
                field_type: FieldType::Enum {
                    inner: Box::new(u16_t.clone()),
                    variants: vec![
                        (0, "ET_NONE".into()),
                        (1, "ET_REL".into()),
                        (2, "ET_EXEC".into()),
                        (3, "ET_DYN".into()),
                        (4, "ET_CORE".into()),
                    ],
                },
                description: "Object file type".into(),
                editable: true,
            },
            FieldDef {
                name: "e_machine".into(),
                offset: 18,
                field_type: FieldType::Enum {
                    inner: Box::new(u16_t.clone()),
                    variants: vec![
                        (0x03, "EM_386".into()),
                        (0x3e, "EM_X86_64".into()),
                        (0xb7, "EM_AARCH64".into()),
                        (0xf3, "EM_RISCV".into()),
                    ],
                },
                description: "Architecture".into(),
                editable: true,
            },
            FieldDef {
                name: "e_version".into(),
                offset: 20,
                field_type: u32_t.clone(),
                description: "Object file version".into(),
                editable: true,
            },
        ];

        let (phoff, phnum, phentsize, shoff, shnum, shentsize, shstrndx) = if self.is_64 {
            fields.extend(vec![
                FieldDef {
                    name: "e_entry".into(),
                    offset: 24,
                    field_type: addr_t.clone(),
                    description: "Entry point virtual address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_phoff".into(),
                    offset: 32,
                    field_type: off_t.clone(),
                    description: "Program header table offset".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shoff".into(),
                    offset: 40,
                    field_type: off_t.clone(),
                    description: "Section header table offset".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_flags".into(),
                    offset: 48,
                    field_type: u32_t.clone(),
                    description: "Processor-specific flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_ehsize".into(),
                    offset: 52,
                    field_type: u16_t.clone(),
                    description: "ELF header size".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_phentsize".into(),
                    offset: 54,
                    field_type: u16_t.clone(),
                    description: "Program header entry size".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_phnum".into(),
                    offset: 56,
                    field_type: u16_t.clone(),
                    description: "Number of program headers".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shentsize".into(),
                    offset: 58,
                    field_type: u16_t.clone(),
                    description: "Section header entry size".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shnum".into(),
                    offset: 60,
                    field_type: u16_t.clone(),
                    description: "Number of section headers".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shstrndx".into(),
                    offset: 62,
                    field_type: u16_t.clone(),
                    description: "Section name string table index".into(),
                    editable: true,
                },
            ]);

            (
                self.read_u64(32).unwrap_or(0),
                self.read_u16(56).unwrap_or(0) as usize,
                self.read_u16(54).unwrap_or(0) as u64,
                self.read_u64(40).unwrap_or(0),
                self.read_u16(60).unwrap_or(0) as usize,
                self.read_u16(58).unwrap_or(0) as u64,
                self.read_u16(62).unwrap_or(0) as usize,
            )
        } else {
            fields.extend(vec![
                FieldDef {
                    name: "e_entry".into(),
                    offset: 24,
                    field_type: addr_t.clone(),
                    description: "Entry point virtual address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_phoff".into(),
                    offset: 28,
                    field_type: off_t.clone(),
                    description: "Program header table offset".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shoff".into(),
                    offset: 32,
                    field_type: off_t.clone(),
                    description: "Section header table offset".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_flags".into(),
                    offset: 36,
                    field_type: u32_t.clone(),
                    description: "Processor-specific flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_ehsize".into(),
                    offset: 40,
                    field_type: u16_t.clone(),
                    description: "ELF header size".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_phentsize".into(),
                    offset: 42,
                    field_type: u16_t.clone(),
                    description: "Program header entry size".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_phnum".into(),
                    offset: 44,
                    field_type: u16_t.clone(),
                    description: "Number of program headers".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shentsize".into(),
                    offset: 46,
                    field_type: u16_t.clone(),
                    description: "Section header entry size".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shnum".into(),
                    offset: 48,
                    field_type: u16_t.clone(),
                    description: "Number of section headers".into(),
                    editable: true,
                },
                FieldDef {
                    name: "e_shstrndx".into(),
                    offset: 50,
                    field_type: u16_t.clone(),
                    description: "Section name string table index".into(),
                    editable: true,
                },
            ]);

            (
                self.read_u32(28).unwrap_or(0) as u64,
                self.read_u16(44).unwrap_or(0) as usize,
                self.read_u16(42).unwrap_or(0) as u64,
                self.read_u32(32).unwrap_or(0) as u64,
                self.read_u16(48).unwrap_or(0) as usize,
                self.read_u16(46).unwrap_or(0) as u64,
                self.read_u16(50).unwrap_or(0) as usize,
            )
        };

        Some((
            fields, phoff, phnum, phentsize, shoff, shnum, shentsize, shstrndx,
        ))
    }

    fn parse_program_headers(
        &mut self,
        phoff: u64,
        phnum: usize,
        phentsize: u64,
    ) -> Vec<ProgramHeaderInfo> {
        let expected = if self.is_64 {
            ELF64_PHDR_SIZE
        } else {
            ELF32_PHDR_SIZE
        };
        if phoff == 0 || phnum == 0 || phentsize < expected {
            return Vec::new();
        }

        let mut out = Vec::new();
        for index in 0..phnum {
            let base = phoff.saturating_add(index as u64 * phentsize);
            let Some(p_type) = self.read_u32(base) else {
                break;
            };
            let info = if self.is_64 {
                let Some(flags) = self.read_u32(base + 4) else {
                    break;
                };
                let Some(offset) = self.read_u64(base + 8) else {
                    break;
                };
                let Some(vaddr) = self.read_u64(base + 16) else {
                    break;
                };
                let Some(paddr) = self.read_u64(base + 24) else {
                    break;
                };
                let Some(filesz) = self.read_u64(base + 32) else {
                    break;
                };
                let Some(memsz) = self.read_u64(base + 40) else {
                    break;
                };
                let Some(align) = self.read_u64(base + 48) else {
                    break;
                };
                ProgramHeaderInfo {
                    index,
                    entry_offset: base,
                    p_type,
                    flags,
                    offset,
                    vaddr,
                    paddr,
                    filesz,
                    memsz,
                    align,
                }
            } else {
                let Some(offset) = self.read_u32(base + 4) else {
                    break;
                };
                let Some(vaddr) = self.read_u32(base + 8) else {
                    break;
                };
                let Some(paddr) = self.read_u32(base + 12) else {
                    break;
                };
                let Some(filesz) = self.read_u32(base + 16) else {
                    break;
                };
                let Some(memsz) = self.read_u32(base + 20) else {
                    break;
                };
                let Some(flags) = self.read_u32(base + 24) else {
                    break;
                };
                let Some(align) = self.read_u32(base + 28) else {
                    break;
                };
                ProgramHeaderInfo {
                    index,
                    entry_offset: base,
                    p_type,
                    flags,
                    offset: offset as u64,
                    vaddr: vaddr as u64,
                    paddr: paddr as u64,
                    filesz: filesz as u64,
                    memsz: memsz as u64,
                    align: align as u64,
                }
            };
            out.push(info);
        }
        out
    }

    fn parse_section_headers(
        &mut self,
        shoff: u64,
        shnum: usize,
        shentsize: u64,
        shstrndx: usize,
    ) -> Vec<SectionHeaderInfo> {
        let expected = if self.is_64 {
            ELF64_SHDR_SIZE
        } else {
            ELF32_SHDR_SIZE
        };
        if shoff == 0 || shnum == 0 || shentsize < expected {
            return Vec::new();
        }

        let mut out = Vec::new();
        for index in 0..shnum {
            let base = shoff.saturating_add(index as u64 * shentsize);
            let Some(name_offset) = self.read_u32(base) else {
                break;
            };
            let Some(sh_type) = self.read_u32(base + 4) else {
                break;
            };
            let info = if self.is_64 {
                let Some(flags) = self.read_u64(base + 8) else {
                    break;
                };
                let Some(addr) = self.read_u64(base + 16) else {
                    break;
                };
                let Some(offset) = self.read_u64(base + 24) else {
                    break;
                };
                let Some(size) = self.read_u64(base + 32) else {
                    break;
                };
                let Some(link) = self.read_u32(base + 40) else {
                    break;
                };
                let Some(section_info) = self.read_u32(base + 44) else {
                    break;
                };
                let Some(addralign) = self.read_u64(base + 48) else {
                    break;
                };
                let Some(entsize) = self.read_u64(base + 56) else {
                    break;
                };
                SectionHeaderInfo {
                    index,
                    entry_offset: base,
                    name_offset,
                    name: String::new(),
                    sh_type,
                    flags,
                    addr,
                    offset,
                    size,
                    link,
                    info: section_info,
                    addralign,
                    entsize,
                }
            } else {
                let Some(flags) = self.read_u32(base + 8) else {
                    break;
                };
                let Some(addr) = self.read_u32(base + 12) else {
                    break;
                };
                let Some(offset) = self.read_u32(base + 16) else {
                    break;
                };
                let Some(size) = self.read_u32(base + 20) else {
                    break;
                };
                let Some(link) = self.read_u32(base + 24) else {
                    break;
                };
                let Some(section_info) = self.read_u32(base + 28) else {
                    break;
                };
                let Some(addralign) = self.read_u32(base + 32) else {
                    break;
                };
                let Some(entsize) = self.read_u32(base + 36) else {
                    break;
                };
                SectionHeaderInfo {
                    index,
                    entry_offset: base,
                    name_offset,
                    name: String::new(),
                    sh_type,
                    flags: flags as u64,
                    addr: addr as u64,
                    offset: offset as u64,
                    size: size as u64,
                    link,
                    info: section_info,
                    addralign: addralign as u64,
                    entsize: entsize as u64,
                }
            };
            out.push(info);
        }

        let shstrtab = out.get(shstrndx).cloned();
        for section in &mut out {
            section.name = shstrtab
                .as_ref()
                .and_then(|table| self.read_string_from_table(table, section.name_offset as u64))
                .unwrap_or_default();
        }

        out
    }

    fn build_program_header_table(
        &mut self,
        phoff: u64,
        total_count: usize,
        headers: &[ProgramHeaderInfo],
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let shown = self.shown_count(headers.len());
        let mut children: Vec<StructDef> = headers
            .iter()
            .take(shown)
            .map(|header| self.build_program_header_struct(header, sections))
            .collect();

        if shown < total_count {
            children.push(more_marker(
                format!(
                    "… more program headers beyond {} (use `:insp more` to load more)",
                    shown
                ),
                phoff.saturating_add(
                    shown as u64
                        * if self.is_64 {
                            ELF64_PHDR_SIZE
                        } else {
                            ELF32_PHDR_SIZE
                        },
                ),
            ));
        }

        StructDef {
            name: format!("Program Header Table ({} entries)", total_count),
            base_offset: phoff,
            fields: vec![],
            children,
        }
    }

    fn build_section_header_table(
        &mut self,
        shoff: u64,
        total_count: usize,
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let shown = self.shown_count(sections.len());
        let mut children: Vec<StructDef> = sections
            .iter()
            .take(shown)
            .map(|section| self.build_section_header_struct(section, sections))
            .collect();

        if shown < total_count {
            children.push(more_marker(
                format!(
                    "… more section headers beyond {} (use `:insp more` to load more)",
                    shown
                ),
                shoff.saturating_add(
                    shown as u64
                        * if self.is_64 {
                            ELF64_SHDR_SIZE
                        } else {
                            ELF32_SHDR_SIZE
                        },
                ),
            ));
        }

        StructDef {
            name: format!("Section Header Table ({} entries)", total_count),
            base_offset: shoff,
            fields: vec![],
            children,
        }
    }

    fn build_program_header_struct(
        &mut self,
        header: &ProgramHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let u32_t = self.u32_t();
        let addr_t = self.word_t();

        let mut children = self.build_segment_payload_children(header, sections);
        if header.filesz > 0 && header.offset < self.doc.len() {
            children.push(data_range_struct(
                format!("Segment Data {}", header.index),
                header.offset,
                "segment_data",
                header.filesz,
                "Segment data range in file",
            ));
        }

        StructDef {
            name: format!(
                "Program Header {}: {}",
                header.index,
                program_type_label(header.p_type)
            ),
            base_offset: header.entry_offset,
            fields: vec![
                FieldDef {
                    name: "p_type".into(),
                    offset: 0,
                    field_type: FieldType::Enum {
                        inner: Box::new(u32_t.clone()),
                        variants: program_type_variants(),
                    },
                    description: "Segment type".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_flags".into(),
                    offset: if self.is_64 { 4 } else { 24 },
                    field_type: FieldType::Flags {
                        inner: Box::new(u32_t.clone()),
                        flags: vec![(4, "R".into()), (2, "W".into()), (1, "X".into())],
                    },
                    description: "Segment flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_offset".into(),
                    offset: if self.is_64 { 8 } else { 4 },
                    field_type: addr_t.clone(),
                    description: "File offset of segment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_vaddr".into(),
                    offset: if self.is_64 { 16 } else { 8 },
                    field_type: addr_t.clone(),
                    description: "Virtual address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_paddr".into(),
                    offset: if self.is_64 { 24 } else { 12 },
                    field_type: addr_t.clone(),
                    description: "Physical address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_filesz".into(),
                    offset: if self.is_64 { 32 } else { 16 },
                    field_type: addr_t.clone(),
                    description: "Size in file".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_memsz".into(),
                    offset: if self.is_64 { 40 } else { 20 },
                    field_type: addr_t.clone(),
                    description: "Size in memory".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_align".into(),
                    offset: if self.is_64 { 48 } else { 28 },
                    field_type: addr_t,
                    description: "Alignment".into(),
                    editable: true,
                },
            ],
            children,
        }
    }

    fn build_section_header_struct(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let u32_t = self.u32_t();
        let word_t = self.word_t();

        let mut children = self.build_section_payload_children(section, sections);
        if section.size > 0 && section.sh_type != SHT_NOBITS && section.offset < self.doc.len() {
            children.push(data_range_struct(
                format!("Section Data {}", section.index),
                section.offset,
                "section_data",
                section.size,
                "Section data range in file",
            ));
        }

        StructDef {
            name: format!(
                "Section {}: {} ({})",
                section.index,
                section_display_name(section),
                section_type_label(section.sh_type)
            ),
            base_offset: section.entry_offset,
            fields: vec![
                FieldDef {
                    name: "sh_name".into(),
                    offset: 0,
                    field_type: u32_t.clone(),
                    description: "Offset into the section name string table".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_type".into(),
                    offset: 4,
                    field_type: FieldType::Enum {
                        inner: Box::new(u32_t.clone()),
                        variants: section_type_variants(),
                    },
                    description: "Section type".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_flags".into(),
                    offset: if self.is_64 { 8 } else { 8 },
                    field_type: FieldType::Flags {
                        inner: Box::new(word_t.clone()),
                        flags: section_flag_variants(),
                    },
                    description: "Section attribute flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_addr".into(),
                    offset: if self.is_64 { 16 } else { 12 },
                    field_type: word_t.clone(),
                    description: "Virtual address in memory".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_offset".into(),
                    offset: if self.is_64 { 24 } else { 16 },
                    field_type: word_t.clone(),
                    description: "File offset of section data".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_size".into(),
                    offset: if self.is_64 { 32 } else { 20 },
                    field_type: word_t.clone(),
                    description: "Size of section data".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_link".into(),
                    offset: if self.is_64 { 40 } else { 24 },
                    field_type: u32_t.clone(),
                    description: "Section-specific link".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_info".into(),
                    offset: if self.is_64 { 44 } else { 28 },
                    field_type: u32_t.clone(),
                    description: "Section-specific extra info".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_addralign".into(),
                    offset: if self.is_64 { 48 } else { 32 },
                    field_type: word_t.clone(),
                    description: "Required alignment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_entsize".into(),
                    offset: if self.is_64 { 56 } else { 36 },
                    field_type: word_t,
                    description: "Entry size for table sections".into(),
                    editable: true,
                },
            ],
            children,
        }
    }

    fn build_segment_payload_children(
        &mut self,
        header: &ProgramHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> Vec<StructDef> {
        let mut children = Vec::new();

        if matches!(header.p_type, PT_INTERP) {
            if let Some(interpreter) = self.build_interpreter_struct(
                "Interpreter".to_owned(),
                header.offset,
                header.filesz,
            ) {
                children.push(interpreter);
            }
        }

        if matches!(header.p_type, PT_DYNAMIC) {
            let string_table = find_named_section(sections, ".dynstr");
            if let Some(dynamic) = self.build_dynamic_entries_struct(
                "Dynamic Entries".to_owned(),
                header.offset,
                header.filesz,
                string_table,
            ) {
                children.push(dynamic);
            }
        }

        if matches!(header.p_type, PT_NOTE | PT_GNU_PROPERTY) {
            if let Some(notes) = self.build_notes_struct(
                format!("Notes ({})", program_type_label(header.p_type)),
                header.offset,
                header.filesz,
            ) {
                children.push(notes);
            }
        }

        children
    }

    fn build_section_payload_children(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> Vec<StructDef> {
        let mut children = Vec::new();

        if section.name == ".interp" {
            if let Some(interpreter) = self.build_interpreter_struct(
                "Interpreter".to_owned(),
                section.offset,
                section.size,
            ) {
                children.push(interpreter);
            }
        }

        if matches!(section.sh_type, SHT_DYNAMIC) || section.name == ".dynamic" {
            let string_table = section_link_target(sections, section.link)
                .filter(|linked| linked.sh_type == SHT_STRTAB)
                .or_else(|| find_named_section(sections, ".dynstr"));
            if let Some(dynamic) = self.build_dynamic_entries_struct(
                "Dynamic Entries".to_owned(),
                section.offset,
                section.size,
                string_table,
            ) {
                children.push(dynamic);
            }
        }

        if matches!(section.sh_type, SHT_NOTE) || section.name.starts_with(".note") {
            if let Some(notes) = self.build_notes_struct(
                format!("Notes ({})", section_display_name(section)),
                section.offset,
                section.size,
            ) {
                children.push(notes);
            }
        }

        children
    }

    fn build_interpreter_struct(
        &mut self,
        label: String,
        offset: u64,
        size: u64,
    ) -> Option<StructDef> {
        let (path, consumed) = self.read_c_string(offset, size)?;
        Some(utf8_struct(
            format!("{label}: {path}"),
            offset,
            "path",
            consumed,
            "ELF interpreter path",
        ))
    }

    fn build_dynamic_entries_struct(
        &mut self,
        name: String,
        offset: u64,
        size: u64,
        string_table: Option<&SectionHeaderInfo>,
    ) -> Option<StructDef> {
        let entry_size = if self.is_64 { 16 } else { 8 };
        if size < entry_size || offset >= self.doc.len() {
            return None;
        }

        let mut entries = Vec::new();
        let end = offset.saturating_add(size);
        let mut cursor = offset;
        while cursor.saturating_add(entry_size) <= end {
            let (tag, value) = if self.is_64 {
                (self.read_u64(cursor)?, self.read_u64(cursor + 8)?)
            } else {
                (
                    self.read_u32(cursor)? as u64,
                    self.read_u32(cursor + 4)? as u64,
                )
            };
            entries.push((cursor, tag, value));
            cursor = cursor.saturating_add(entry_size);
            if tag == DT_NULL {
                break;
            }
        }

        if entries.is_empty() {
            return None;
        }

        let word_t = self.word_t();
        let shown = self.shown_count(entries.len());
        let mut children = Vec::new();
        for (index, (entry_offset, tag, value)) in entries.iter().take(shown).enumerate() {
            let mut entry_children = Vec::new();
            let mut label = dynamic_tag_label(*tag).to_owned();
            if let Some(table) = string_table.filter(|_| dynamic_tag_uses_string(*tag)) {
                if let Some((text, len)) = self.string_struct(table, *value) {
                    label = format!("{label} -> {text}");
                    let string_offset = table.offset.saturating_add(*value);
                    entry_children.push(utf8_struct(
                        format!("String: {text}"),
                        string_offset,
                        "value",
                        len,
                        "Dynamic string value",
                    ));
                }
            }

            children.push(StructDef {
                name: format!("Dynamic {}: {}", index, label),
                base_offset: *entry_offset,
                fields: vec![
                    FieldDef {
                        name: "d_tag".into(),
                        offset: 0,
                        field_type: FieldType::Enum {
                            inner: Box::new(word_t.clone()),
                            variants: dynamic_tag_variants(),
                        },
                        description: "Dynamic entry tag".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "d_val".into(),
                        offset: if self.is_64 { 8 } else { 4 },
                        field_type: word_t.clone(),
                        description: "Dynamic entry value or pointer".into(),
                        editable: false,
                    },
                ],
                children: entry_children,
            });
        }

        if shown < entries.len() {
            children.push(more_marker(
                format!(
                    "… more dynamic entries beyond {} (use `:insp more` to load more)",
                    shown
                ),
                entries[shown].0,
            ));
        }

        Some(StructDef {
            name,
            base_offset: offset,
            fields: vec![],
            children,
        })
    }

    fn build_notes_struct(&mut self, name: String, offset: u64, size: u64) -> Option<StructDef> {
        if size < 12 || offset >= self.doc.len() {
            return None;
        }

        let mut notes = Vec::new();
        let end = offset.saturating_add(size);
        let mut cursor = offset;
        while cursor.saturating_add(12) <= end {
            let Some(namesz) = self.read_u32(cursor) else {
                break;
            };
            let Some(descsz) = self.read_u32(cursor + 4) else {
                break;
            };
            let Some(note_type) = self.read_u32(cursor + 8) else {
                break;
            };
            let name_offset = cursor + 12;
            let name_padded = align_up(namesz as u64, 4);
            let desc_offset = name_offset.saturating_add(name_padded);
            let desc_padded = align_up(descsz as u64, 4);
            let next = desc_offset.saturating_add(desc_padded);
            if desc_offset.saturating_add(descsz as u64) > end || next <= cursor {
                break;
            }

            let note_name = if namesz == 0 {
                String::new()
            } else {
                self.read_c_string(name_offset, namesz as u64)
                    .map(|(text, _)| text)
                    .unwrap_or_default()
            };
            notes.push((
                cursor,
                namesz,
                descsz,
                note_type,
                note_name,
                name_offset,
                desc_offset,
            ));

            cursor = next;
            if cursor >= end {
                break;
            }
        }

        if notes.is_empty() {
            return None;
        }

        let shown = self.shown_count(notes.len());
        let mut children = Vec::new();
        for (
            index,
            (entry_offset, namesz, descsz, note_type, note_name, name_offset, desc_offset),
        ) in notes.iter().take(shown).enumerate()
        {
            let mut note_children = Vec::new();
            if *namesz > 0 {
                let (_, len) = self
                    .read_c_string(*name_offset, *namesz as u64)
                    .unwrap_or_default();
                if len > 0 {
                    note_children.push(utf8_struct(
                        format!("Note Name: {}", note_name),
                        *name_offset,
                        "name",
                        len,
                        "ELF note name",
                    ));
                }
            }

            if *descsz > 0 {
                if note_name == "GNU" && *note_type == NT_GNU_PROPERTY_TYPE_0 {
                    if let Some(properties) =
                        self.build_gnu_properties_struct(*desc_offset, *descsz as u64)
                    {
                        note_children.push(properties);
                    }
                } else {
                    note_children.push(data_range_struct(
                        format!("Note Descriptor {}", index),
                        *desc_offset,
                        "desc_data",
                        *descsz as u64,
                        "ELF note descriptor bytes",
                    ));
                }
            }

            children.push(StructDef {
                name: format!(
                    "Note {}: {} ({})",
                    index,
                    if note_name.is_empty() {
                        "<anon>"
                    } else {
                        note_name.as_str()
                    },
                    note_type_label(note_name, *note_type)
                ),
                base_offset: *entry_offset,
                fields: vec![
                    FieldDef {
                        name: "n_namesz".into(),
                        offset: 0,
                        field_type: self.u32_t(),
                        description: "Name size".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "n_descsz".into(),
                        offset: 4,
                        field_type: self.u32_t(),
                        description: "Descriptor size".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "n_type".into(),
                        offset: 8,
                        field_type: self.u32_t(),
                        description: "Note type".into(),
                        editable: false,
                    },
                ],
                children: note_children,
            });
        }

        if shown < notes.len() {
            children.push(more_marker(
                format!(
                    "… more notes beyond {} (use `:insp more` to load more)",
                    shown
                ),
                notes[shown].0,
            ));
        }

        Some(StructDef {
            name,
            base_offset: offset,
            fields: vec![],
            children,
        })
    }

    fn build_gnu_properties_struct(&mut self, offset: u64, size: u64) -> Option<StructDef> {
        if size < 8 || offset >= self.doc.len() {
            return None;
        }

        let align = if self.is_64 { 8 } else { 4 };
        let end = offset.saturating_add(size);
        let mut properties = Vec::new();
        let mut cursor = offset;
        while cursor.saturating_add(8) <= end {
            let Some(prop_type) = self.read_u32(cursor) else {
                break;
            };
            let Some(data_size) = self.read_u32(cursor + 4) else {
                break;
            };
            let data_offset = cursor + 8;
            if data_offset.saturating_add(data_size as u64) > end {
                break;
            }
            properties.push((cursor, prop_type, data_size, data_offset));
            let next = cursor.saturating_add(align_up(8 + data_size as u64, align));
            if next <= cursor {
                break;
            }
            cursor = next;
        }

        if properties.is_empty() {
            return None;
        }

        let shown = self.shown_count(properties.len());
        let mut children = Vec::new();
        for (index, (entry_offset, prop_type, data_size, data_offset)) in
            properties.iter().take(shown).enumerate()
        {
            let mut property_children = Vec::new();
            if *data_size > 0 {
                property_children.push(data_range_struct(
                    format!("Property Data {}", index),
                    *data_offset,
                    "pr_data",
                    *data_size as u64,
                    "GNU property payload",
                ));
            }

            children.push(StructDef {
                name: format!("Property {}: {}", index, gnu_property_label(*prop_type)),
                base_offset: *entry_offset,
                fields: vec![
                    FieldDef {
                        name: "pr_type".into(),
                        offset: 0,
                        field_type: FieldType::Enum {
                            inner: Box::new(self.u32_t()),
                            variants: gnu_property_variants(),
                        },
                        description: "GNU property type".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "pr_datasz".into(),
                        offset: 4,
                        field_type: self.u32_t(),
                        description: "GNU property payload size".into(),
                        editable: false,
                    },
                ],
                children: property_children,
            });
        }

        if shown < properties.len() {
            children.push(more_marker(
                format!(
                    "… more GNU properties beyond {} (use `:insp more` to load more)",
                    shown
                ),
                properties[shown].0,
            ));
        }

        Some(StructDef {
            name: "GNU Properties".to_owned(),
            base_offset: offset,
            fields: vec![],
            children,
        })
    }

    fn string_struct(&mut self, table: &SectionHeaderInfo, offset: u64) -> Option<(String, usize)> {
        if table.sh_type != SHT_STRTAB || offset >= table.size {
            return None;
        }
        let abs = table.offset.checked_add(offset)?;
        self.read_c_string(abs, table.size.saturating_sub(offset))
    }

    fn read_string_from_table(&mut self, table: &SectionHeaderInfo, offset: u64) -> Option<String> {
        if table.sh_type != SHT_STRTAB || offset >= table.size {
            return None;
        }
        let abs = table.offset.checked_add(offset)?;
        let remaining = table.size.saturating_sub(offset);
        self.read_c_string(abs, remaining).map(|(text, _)| text)
    }

    fn read_c_string(&mut self, offset: u64, max_len: u64) -> Option<(String, usize)> {
        if max_len == 0 || offset >= self.doc.len() {
            return None;
        }
        let capped = max_len.min(4096) as usize;
        let bytes = self.doc.read_logical_range(offset, capped).ok()?;
        if bytes.is_empty() {
            return None;
        }
        let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let consumed = if nul < bytes.len() {
            nul + 1
        } else {
            bytes.len()
        };
        Some((
            String::from_utf8_lossy(&bytes[..nul]).into_owned(),
            consumed,
        ))
    }

    fn shown_count(&self, total: usize) -> usize {
        total.min(self.entry_cap.max(1))
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

fn data_range_struct(
    name: String,
    base_offset: u64,
    field_name: &str,
    len: u64,
    description: &str,
) -> StructDef {
    StructDef {
        name,
        base_offset,
        fields: vec![FieldDef {
            name: field_name.to_string(),
            offset: 0,
            field_type: FieldType::DataRange(len),
            description: description.to_string(),
            editable: false,
        }],
        children: vec![],
    }
}

fn utf8_struct(
    name: String,
    base_offset: u64,
    field_name: &str,
    len: usize,
    description: &str,
) -> StructDef {
    StructDef {
        name,
        base_offset,
        fields: vec![FieldDef {
            name: field_name.to_string(),
            offset: 0,
            field_type: FieldType::Utf8(len),
            description: description.to_string(),
            editable: false,
        }],
        children: vec![],
    }
}

fn more_marker(name: String, base_offset: u64) -> StructDef {
    StructDef {
        name,
        base_offset,
        fields: vec![],
        children: vec![],
    }
}

fn section_display_name(section: &SectionHeaderInfo) -> &str {
    if section.name.is_empty() {
        match section.sh_type {
            SHT_NULL => "<null>",
            _ => "<unnamed>",
        }
    } else {
        &section.name
    }
}

fn align_up(value: u64, align: u64) -> u64 {
    if align <= 1 {
        value
    } else {
        value.saturating_add(align - 1) / align * align
    }
}

fn section_link_target(sections: &[SectionHeaderInfo], link: u32) -> Option<&SectionHeaderInfo> {
    sections.get(link as usize)
}

fn find_named_section<'a>(
    sections: &'a [SectionHeaderInfo],
    name: &str,
) -> Option<&'a SectionHeaderInfo> {
    sections.iter().find(|section| section.name == name)
}

fn program_type_variants() -> Vec<(u64, String)> {
    vec![
        (PT_NULL as u64, "PT_NULL".into()),
        (PT_LOAD as u64, "PT_LOAD".into()),
        (PT_DYNAMIC as u64, "PT_DYNAMIC".into()),
        (PT_INTERP as u64, "PT_INTERP".into()),
        (PT_NOTE as u64, "PT_NOTE".into()),
        (PT_SHLIB as u64, "PT_SHLIB".into()),
        (PT_PHDR as u64, "PT_PHDR".into()),
        (PT_TLS as u64, "PT_TLS".into()),
        (PT_GNU_EH_FRAME as u64, "PT_GNU_EH_FRAME".into()),
        (PT_GNU_STACK as u64, "PT_GNU_STACK".into()),
        (PT_GNU_RELRO as u64, "PT_GNU_RELRO".into()),
        (PT_GNU_PROPERTY as u64, "PT_GNU_PROPERTY".into()),
    ]
}

fn section_type_variants() -> Vec<(u64, String)> {
    vec![
        (SHT_NULL as u64, "SHT_NULL".into()),
        (SHT_PROGBITS as u64, "SHT_PROGBITS".into()),
        (SHT_SYMTAB as u64, "SHT_SYMTAB".into()),
        (SHT_STRTAB as u64, "SHT_STRTAB".into()),
        (SHT_RELA as u64, "SHT_RELA".into()),
        (SHT_HASH as u64, "SHT_HASH".into()),
        (SHT_DYNAMIC as u64, "SHT_DYNAMIC".into()),
        (SHT_NOTE as u64, "SHT_NOTE".into()),
        (SHT_NOBITS as u64, "SHT_NOBITS".into()),
        (SHT_REL as u64, "SHT_REL".into()),
        (SHT_DYNSYM as u64, "SHT_DYNSYM".into()),
        (SHT_GNU_HASH as u64, "SHT_GNU_HASH".into()),
        (SHT_GNU_VERDEF as u64, "SHT_GNU_VERDEF".into()),
        (SHT_GNU_VERNEED as u64, "SHT_GNU_VERNEED".into()),
        (SHT_GNU_VERSYM as u64, "SHT_GNU_VERSYM".into()),
    ]
}

fn section_flag_variants() -> Vec<(u64, String)> {
    vec![
        (0x1, "WRITE".into()),
        (0x2, "ALLOC".into()),
        (0x4, "EXECINSTR".into()),
        (0x10, "MERGE".into()),
        (0x20, "STRINGS".into()),
        (0x40, "INFO_LINK".into()),
        (0x80, "LINK_ORDER".into()),
        (0x100, "OS_NONCONFORMING".into()),
        (0x200, "GROUP".into()),
        (0x400, "TLS".into()),
    ]
}

fn dynamic_tag_variants() -> Vec<(u64, String)> {
    vec![
        (DT_NULL, "DT_NULL".into()),
        (DT_NEEDED, "DT_NEEDED".into()),
        (DT_PLTRELSZ, "DT_PLTRELSZ".into()),
        (DT_PLTGOT, "DT_PLTGOT".into()),
        (DT_HASH, "DT_HASH".into()),
        (DT_STRTAB, "DT_STRTAB".into()),
        (DT_SYMTAB, "DT_SYMTAB".into()),
        (DT_RELA, "DT_RELA".into()),
        (DT_RELASZ, "DT_RELASZ".into()),
        (DT_RELAENT, "DT_RELAENT".into()),
        (DT_STRSZ, "DT_STRSZ".into()),
        (DT_SYMENT, "DT_SYMENT".into()),
        (DT_INIT, "DT_INIT".into()),
        (DT_FINI, "DT_FINI".into()),
        (DT_SONAME, "DT_SONAME".into()),
        (DT_RPATH, "DT_RPATH".into()),
        (DT_SYMBOLIC, "DT_SYMBOLIC".into()),
        (DT_REL, "DT_REL".into()),
        (DT_RELSZ, "DT_RELSZ".into()),
        (DT_RELENT, "DT_RELENT".into()),
        (DT_PLTREL, "DT_PLTREL".into()),
        (DT_DEBUG, "DT_DEBUG".into()),
        (DT_TEXTREL, "DT_TEXTREL".into()),
        (DT_JMPREL, "DT_JMPREL".into()),
        (DT_BIND_NOW, "DT_BIND_NOW".into()),
        (DT_INIT_ARRAY, "DT_INIT_ARRAY".into()),
        (DT_FINI_ARRAY, "DT_FINI_ARRAY".into()),
        (DT_INIT_ARRAYSZ, "DT_INIT_ARRAYSZ".into()),
        (DT_FINI_ARRAYSZ, "DT_FINI_ARRAYSZ".into()),
        (DT_RUNPATH, "DT_RUNPATH".into()),
        (DT_FLAGS, "DT_FLAGS".into()),
        (DT_PREINIT_ARRAY, "DT_PREINIT_ARRAY".into()),
        (DT_PREINIT_ARRAYSZ, "DT_PREINIT_ARRAYSZ".into()),
        (DT_SYMTAB_SHNDX, "DT_SYMTAB_SHNDX".into()),
        (DT_GNU_HASH, "DT_GNU_HASH".into()),
        (DT_FLAGS_1, "DT_FLAGS_1".into()),
        (DT_VERDEF, "DT_VERDEF".into()),
        (DT_VERDEFNUM, "DT_VERDEFNUM".into()),
        (DT_VERNEED, "DT_VERNEED".into()),
        (DT_VERNEEDNUM, "DT_VERNEEDNUM".into()),
    ]
}

fn dynamic_tag_label(tag: u64) -> &'static str {
    match tag {
        DT_NULL => "DT_NULL",
        DT_NEEDED => "DT_NEEDED",
        DT_PLTRELSZ => "DT_PLTRELSZ",
        DT_PLTGOT => "DT_PLTGOT",
        DT_HASH => "DT_HASH",
        DT_STRTAB => "DT_STRTAB",
        DT_SYMTAB => "DT_SYMTAB",
        DT_RELA => "DT_RELA",
        DT_RELASZ => "DT_RELASZ",
        DT_RELAENT => "DT_RELAENT",
        DT_STRSZ => "DT_STRSZ",
        DT_SYMENT => "DT_SYMENT",
        DT_INIT => "DT_INIT",
        DT_FINI => "DT_FINI",
        DT_SONAME => "DT_SONAME",
        DT_RPATH => "DT_RPATH",
        DT_SYMBOLIC => "DT_SYMBOLIC",
        DT_REL => "DT_REL",
        DT_RELSZ => "DT_RELSZ",
        DT_RELENT => "DT_RELENT",
        DT_PLTREL => "DT_PLTREL",
        DT_DEBUG => "DT_DEBUG",
        DT_TEXTREL => "DT_TEXTREL",
        DT_JMPREL => "DT_JMPREL",
        DT_BIND_NOW => "DT_BIND_NOW",
        DT_INIT_ARRAY => "DT_INIT_ARRAY",
        DT_FINI_ARRAY => "DT_FINI_ARRAY",
        DT_INIT_ARRAYSZ => "DT_INIT_ARRAYSZ",
        DT_FINI_ARRAYSZ => "DT_FINI_ARRAYSZ",
        DT_RUNPATH => "DT_RUNPATH",
        DT_FLAGS => "DT_FLAGS",
        DT_PREINIT_ARRAY => "DT_PREINIT_ARRAY",
        DT_PREINIT_ARRAYSZ => "DT_PREINIT_ARRAYSZ",
        DT_SYMTAB_SHNDX => "DT_SYMTAB_SHNDX",
        DT_GNU_HASH => "DT_GNU_HASH",
        DT_FLAGS_1 => "DT_FLAGS_1",
        DT_VERDEF => "DT_VERDEF",
        DT_VERDEFNUM => "DT_VERDEFNUM",
        DT_VERNEED => "DT_VERNEED",
        DT_VERNEEDNUM => "DT_VERNEEDNUM",
        _ => "UNKNOWN",
    }
}

fn dynamic_tag_uses_string(tag: u64) -> bool {
    matches!(tag, DT_NEEDED | DT_SONAME | DT_RPATH | DT_RUNPATH)
}

fn program_type_label(p_type: u32) -> &'static str {
    match p_type {
        PT_NULL => "PT_NULL",
        PT_LOAD => "PT_LOAD",
        PT_DYNAMIC => "PT_DYNAMIC",
        PT_INTERP => "PT_INTERP",
        PT_NOTE => "PT_NOTE",
        PT_SHLIB => "PT_SHLIB",
        PT_PHDR => "PT_PHDR",
        PT_TLS => "PT_TLS",
        PT_GNU_EH_FRAME => "PT_GNU_EH_FRAME",
        PT_GNU_STACK => "PT_GNU_STACK",
        PT_GNU_RELRO => "PT_GNU_RELRO",
        PT_GNU_PROPERTY => "PT_GNU_PROPERTY",
        _ => "UNKNOWN",
    }
}

fn section_type_label(sh_type: u32) -> &'static str {
    match sh_type {
        SHT_NULL => "SHT_NULL",
        SHT_PROGBITS => "SHT_PROGBITS",
        SHT_SYMTAB => "SHT_SYMTAB",
        SHT_STRTAB => "SHT_STRTAB",
        SHT_RELA => "SHT_RELA",
        SHT_HASH => "SHT_HASH",
        SHT_DYNAMIC => "SHT_DYNAMIC",
        SHT_NOTE => "SHT_NOTE",
        SHT_NOBITS => "SHT_NOBITS",
        SHT_REL => "SHT_REL",
        SHT_DYNSYM => "SHT_DYNSYM",
        SHT_GNU_HASH => "SHT_GNU_HASH",
        SHT_GNU_VERDEF => "SHT_GNU_VERDEF",
        SHT_GNU_VERNEED => "SHT_GNU_VERNEED",
        SHT_GNU_VERSYM => "SHT_GNU_VERSYM",
        _ => "UNKNOWN",
    }
}

fn note_type_label(note_name: &str, note_type: u32) -> &'static str {
    match (note_name, note_type) {
        ("GNU", NT_GNU_BUILD_ID) => "NT_GNU_BUILD_ID",
        ("GNU", NT_GNU_PROPERTY_TYPE_0) => "NT_GNU_PROPERTY_TYPE_0",
        _ => "UNKNOWN",
    }
}

fn gnu_property_variants() -> Vec<(u64, String)> {
    vec![
        (
            GNU_PROPERTY_STACK_SIZE as u64,
            "GNU_PROPERTY_STACK_SIZE".into(),
        ),
        (
            GNU_PROPERTY_NO_COPY_ON_PROTECTED as u64,
            "GNU_PROPERTY_NO_COPY_ON_PROTECTED".into(),
        ),
        (
            GNU_PROPERTY_X86_FEATURE_1_AND as u64,
            "GNU_PROPERTY_X86_FEATURE_1_AND".into(),
        ),
        (
            GNU_PROPERTY_X86_ISA_1_USED as u64,
            "GNU_PROPERTY_X86_ISA_1_USED".into(),
        ),
        (
            GNU_PROPERTY_X86_ISA_1_NEEDED as u64,
            "GNU_PROPERTY_X86_ISA_1_NEEDED".into(),
        ),
        (
            GNU_PROPERTY_AARCH64_FEATURE_1_AND as u64,
            "GNU_PROPERTY_AARCH64_FEATURE_1_AND".into(),
        ),
    ]
}

fn gnu_property_label(prop_type: u32) -> &'static str {
    match prop_type {
        GNU_PROPERTY_STACK_SIZE => "GNU_PROPERTY_STACK_SIZE",
        GNU_PROPERTY_NO_COPY_ON_PROTECTED => "GNU_PROPERTY_NO_COPY_ON_PROTECTED",
        GNU_PROPERTY_X86_FEATURE_1_AND => "GNU_PROPERTY_X86_FEATURE_1_AND",
        GNU_PROPERTY_X86_ISA_1_USED => "GNU_PROPERTY_X86_ISA_1_USED",
        GNU_PROPERTY_X86_ISA_1_NEEDED => "GNU_PROPERTY_X86_ISA_1_NEEDED",
        GNU_PROPERTY_AARCH64_FEATURE_1_AND => "GNU_PROPERTY_AARCH64_FEATURE_1_AND",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        detect_with_cap, DT_NEEDED, DT_NULL, DT_SONAME, ELF64_EHDR_SIZE, ELF64_PHDR_SIZE,
        ELF64_SHDR_SIZE, ELF_MAGIC, GNU_PROPERTY_X86_FEATURE_1_AND, NT_GNU_PROPERTY_TYPE_0,
        PT_DYNAMIC, PT_GNU_PROPERTY, PT_INTERP, PT_LOAD, PT_NOTE, SHT_DYNAMIC, SHT_NOTE,
        SHT_PROGBITS, SHT_STRTAB,
    };
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format;
    use crate::format::parse::StructValue;

    const HEADER_SIZE: usize = ELF64_EHDR_SIZE as usize;
    const PHDR_OFFSET: usize = HEADER_SIZE;
    const PHDR_SIZE: usize = ELF64_PHDR_SIZE as usize;
    const TEXT_OFFSET: usize = 0x100;
    const SHSTRTAB_OFFSET: usize = 0x120;
    const SHDR_OFFSET: usize = 0x200;
    const SHDR_SIZE: usize = ELF64_SHDR_SIZE as usize;

    fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
        buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
        buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64_le(buf: &mut [u8], offset: usize, value: u64) {
        buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn build_elf64_with_sections(extra_names: &[&str]) -> Vec<u8> {
        let mut strtab = vec![0_u8];
        let mut names = Vec::new();
        for name in [".shstrtab", ".text"]
            .into_iter()
            .chain(extra_names.iter().copied())
        {
            let start = strtab.len() as u32;
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
            names.push((name, start));
        }

        let section_count = 1 + names.len();
        let total_len = SHDR_OFFSET + section_count * SHDR_SIZE;
        let mut bytes = vec![0_u8; total_len.max(SHSTRTAB_OFFSET + strtab.len())];

        bytes[0..4].copy_from_slice(&ELF_MAGIC);
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;

        write_u16_le(&mut bytes, 16, 2);
        write_u16_le(&mut bytes, 18, 0x3e);
        write_u32_le(&mut bytes, 20, 1);
        write_u64_le(&mut bytes, 24, 0x401000);
        write_u64_le(&mut bytes, 32, PHDR_OFFSET as u64);
        write_u64_le(&mut bytes, 40, SHDR_OFFSET as u64);
        write_u32_le(&mut bytes, 48, 0);
        write_u16_le(&mut bytes, 52, HEADER_SIZE as u16);
        write_u16_le(&mut bytes, 54, PHDR_SIZE as u16);
        write_u16_le(&mut bytes, 56, 1);
        write_u16_le(&mut bytes, 58, SHDR_SIZE as u16);
        write_u16_le(&mut bytes, 60, section_count as u16);
        write_u16_le(&mut bytes, 62, 1);

        write_u32_le(&mut bytes, PHDR_OFFSET, PT_LOAD);
        write_u32_le(&mut bytes, PHDR_OFFSET + 4, 0x5);
        write_u64_le(&mut bytes, PHDR_OFFSET + 8, TEXT_OFFSET as u64);
        write_u64_le(&mut bytes, PHDR_OFFSET + 16, 0x401000);
        write_u64_le(&mut bytes, PHDR_OFFSET + 24, 0x401000);
        write_u64_le(&mut bytes, PHDR_OFFSET + 32, 4);
        write_u64_le(&mut bytes, PHDR_OFFSET + 40, 4);
        write_u64_le(&mut bytes, PHDR_OFFSET + 48, 0x1000);

        bytes[TEXT_OFFSET..TEXT_OFFSET + 4].copy_from_slice(&[0x90, 0x90, 0x90, 0xc3]);
        bytes[SHSTRTAB_OFFSET..SHSTRTAB_OFFSET + strtab.len()].copy_from_slice(&strtab);

        let shstrtab_name = names[0].1;
        let text_name = names[1].1;

        write_u32_le(&mut bytes, SHDR_OFFSET + SHDR_SIZE, shstrtab_name);
        write_u32_le(&mut bytes, SHDR_OFFSET + SHDR_SIZE + 4, SHT_STRTAB);
        write_u64_le(
            &mut bytes,
            SHDR_OFFSET + SHDR_SIZE + 24,
            SHSTRTAB_OFFSET as u64,
        );
        write_u64_le(
            &mut bytes,
            SHDR_OFFSET + SHDR_SIZE + 32,
            strtab.len() as u64,
        );
        write_u64_le(&mut bytes, SHDR_OFFSET + SHDR_SIZE + 48, 1);

        let text_header = SHDR_OFFSET + SHDR_SIZE * 2;
        write_u32_le(&mut bytes, text_header, text_name);
        write_u32_le(&mut bytes, text_header + 4, SHT_PROGBITS);
        write_u64_le(&mut bytes, text_header + 8, 0x6);
        write_u64_le(&mut bytes, text_header + 16, 0x401000);
        write_u64_le(&mut bytes, text_header + 24, TEXT_OFFSET as u64);
        write_u64_le(&mut bytes, text_header + 32, 4);
        write_u64_le(&mut bytes, text_header + 48, 16);

        for (slot, (_, name_offset)) in names.iter().skip(2).enumerate() {
            let header = SHDR_OFFSET + SHDR_SIZE * (slot + 3);
            write_u32_le(&mut bytes, header, *name_offset);
            write_u32_le(&mut bytes, header + 4, SHT_PROGBITS);
            write_u64_le(&mut bytes, header + 8, 0x2);
            write_u64_le(&mut bytes, header + 48, 1);
        }

        bytes
    }

    fn build_elf64_with_dynamic_and_notes() -> Vec<u8> {
        const INTERP_OFFSET: usize = 0x180;
        const DYNSTR_OFFSET: usize = 0x1b0;
        const DYNAMIC_OFFSET: usize = 0x1d0;
        const NOTE_OFFSET: usize = 0x200;
        const SHSTRTAB_OFFSET: usize = 0x240;
        const SHDR_OFFSET: usize = 0x2c0;

        let interp = b"/lib64/ld-linux-x86-64.so.2\0";
        let dynstr = b"\0libc.so.6\0sample\0";
        let dynamic_size = 48;

        let note_bytes = {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&4_u32.to_le_bytes());
            bytes.extend_from_slice(&16_u32.to_le_bytes());
            bytes.extend_from_slice(&NT_GNU_PROPERTY_TYPE_0.to_le_bytes());
            bytes.extend_from_slice(b"GNU\0");
            bytes.extend_from_slice(&GNU_PROPERTY_X86_FEATURE_1_AND.to_le_bytes());
            bytes.extend_from_slice(&4_u32.to_le_bytes());
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.extend_from_slice(&[0_u8; 4]);
            bytes
        };

        let mut shstrtab = vec![0_u8];
        let mut name_offsets = Vec::new();
        for name in [
            ".shstrtab",
            ".interp",
            ".dynstr",
            ".dynamic",
            ".note.gnu.property",
        ] {
            let start = shstrtab.len() as u32;
            shstrtab.extend_from_slice(name.as_bytes());
            shstrtab.push(0);
            name_offsets.push(start);
        }

        let section_count = 6;
        let total_len = SHDR_OFFSET + section_count * SHDR_SIZE;
        let mut bytes = vec![0_u8; total_len.max(SHSTRTAB_OFFSET + shstrtab.len())];

        bytes[0..4].copy_from_slice(&ELF_MAGIC);
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;

        write_u16_le(&mut bytes, 16, 3);
        write_u16_le(&mut bytes, 18, 0x3e);
        write_u32_le(&mut bytes, 20, 1);
        write_u64_le(&mut bytes, 32, PHDR_OFFSET as u64);
        write_u64_le(&mut bytes, 40, SHDR_OFFSET as u64);
        write_u16_le(&mut bytes, 52, HEADER_SIZE as u16);
        write_u16_le(&mut bytes, 54, PHDR_SIZE as u16);
        write_u16_le(&mut bytes, 56, 4);
        write_u16_le(&mut bytes, 58, SHDR_SIZE as u16);
        write_u16_le(&mut bytes, 60, section_count as u16);
        write_u16_le(&mut bytes, 62, 1);

        bytes[INTERP_OFFSET..INTERP_OFFSET + interp.len()].copy_from_slice(interp);
        bytes[DYNSTR_OFFSET..DYNSTR_OFFSET + dynstr.len()].copy_from_slice(dynstr);
        bytes[NOTE_OFFSET..NOTE_OFFSET + note_bytes.len()].copy_from_slice(&note_bytes);
        bytes[SHSTRTAB_OFFSET..SHSTRTAB_OFFSET + shstrtab.len()].copy_from_slice(&shstrtab);

        // PT_INTERP
        write_u32_le(&mut bytes, PHDR_OFFSET, PT_INTERP);
        write_u32_le(&mut bytes, PHDR_OFFSET + 4, 0x4);
        write_u64_le(&mut bytes, PHDR_OFFSET + 8, INTERP_OFFSET as u64);
        write_u64_le(&mut bytes, PHDR_OFFSET + 32, interp.len() as u64);
        write_u64_le(&mut bytes, PHDR_OFFSET + 40, interp.len() as u64);
        write_u64_le(&mut bytes, PHDR_OFFSET + 48, 1);

        // PT_DYNAMIC
        let dyn_ph = PHDR_OFFSET + PHDR_SIZE;
        write_u32_le(&mut bytes, dyn_ph, PT_DYNAMIC);
        write_u32_le(&mut bytes, dyn_ph + 4, 0x6);
        write_u64_le(&mut bytes, dyn_ph + 8, DYNAMIC_OFFSET as u64);
        write_u64_le(&mut bytes, dyn_ph + 32, dynamic_size as u64);
        write_u64_le(&mut bytes, dyn_ph + 40, dynamic_size as u64);
        write_u64_le(&mut bytes, dyn_ph + 48, 8);

        // PT_NOTE
        let note_ph = PHDR_OFFSET + PHDR_SIZE * 2;
        write_u32_le(&mut bytes, note_ph, PT_NOTE);
        write_u32_le(&mut bytes, note_ph + 4, 0x4);
        write_u64_le(&mut bytes, note_ph + 8, NOTE_OFFSET as u64);
        write_u64_le(&mut bytes, note_ph + 32, note_bytes.len() as u64);
        write_u64_le(&mut bytes, note_ph + 40, note_bytes.len() as u64);
        write_u64_le(&mut bytes, note_ph + 48, 4);

        // PT_GNU_PROPERTY
        let prop_ph = PHDR_OFFSET + PHDR_SIZE * 3;
        write_u32_le(&mut bytes, prop_ph, PT_GNU_PROPERTY);
        write_u32_le(&mut bytes, prop_ph + 4, 0x4);
        write_u64_le(&mut bytes, prop_ph + 8, NOTE_OFFSET as u64);
        write_u64_le(&mut bytes, prop_ph + 32, note_bytes.len() as u64);
        write_u64_le(&mut bytes, prop_ph + 40, note_bytes.len() as u64);
        write_u64_le(&mut bytes, prop_ph + 48, 8);

        // .dynamic entries
        write_u64_le(&mut bytes, DYNAMIC_OFFSET, DT_NEEDED);
        write_u64_le(&mut bytes, DYNAMIC_OFFSET + 8, 1);
        write_u64_le(&mut bytes, DYNAMIC_OFFSET + 16, DT_SONAME);
        write_u64_le(&mut bytes, DYNAMIC_OFFSET + 24, 11);
        write_u64_le(&mut bytes, DYNAMIC_OFFSET + 32, DT_NULL);
        write_u64_le(&mut bytes, DYNAMIC_OFFSET + 40, 0);

        // Section headers
        let shstrtab_sh = SHDR_OFFSET + SHDR_SIZE;
        write_u32_le(&mut bytes, shstrtab_sh, name_offsets[0]);
        write_u32_le(&mut bytes, shstrtab_sh + 4, SHT_STRTAB);
        write_u64_le(&mut bytes, shstrtab_sh + 24, SHSTRTAB_OFFSET as u64);
        write_u64_le(&mut bytes, shstrtab_sh + 32, shstrtab.len() as u64);
        write_u64_le(&mut bytes, shstrtab_sh + 48, 1);

        let interp_sh = SHDR_OFFSET + SHDR_SIZE * 2;
        write_u32_le(&mut bytes, interp_sh, name_offsets[1]);
        write_u32_le(&mut bytes, interp_sh + 4, SHT_PROGBITS);
        write_u64_le(&mut bytes, interp_sh + 8, 0x2);
        write_u64_le(&mut bytes, interp_sh + 24, INTERP_OFFSET as u64);
        write_u64_le(&mut bytes, interp_sh + 32, interp.len() as u64);
        write_u64_le(&mut bytes, interp_sh + 48, 1);

        let dynstr_sh = SHDR_OFFSET + SHDR_SIZE * 3;
        write_u32_le(&mut bytes, dynstr_sh, name_offsets[2]);
        write_u32_le(&mut bytes, dynstr_sh + 4, SHT_STRTAB);
        write_u64_le(&mut bytes, dynstr_sh + 8, 0x2);
        write_u64_le(&mut bytes, dynstr_sh + 24, DYNSTR_OFFSET as u64);
        write_u64_le(&mut bytes, dynstr_sh + 32, dynstr.len() as u64);
        write_u64_le(&mut bytes, dynstr_sh + 48, 1);

        let dynamic_sh = SHDR_OFFSET + SHDR_SIZE * 4;
        write_u32_le(&mut bytes, dynamic_sh, name_offsets[3]);
        write_u32_le(&mut bytes, dynamic_sh + 4, SHT_DYNAMIC);
        write_u64_le(&mut bytes, dynamic_sh + 8, 0x3);
        write_u64_le(&mut bytes, dynamic_sh + 24, DYNAMIC_OFFSET as u64);
        write_u64_le(&mut bytes, dynamic_sh + 32, dynamic_size as u64);
        write_u32_le(&mut bytes, dynamic_sh + 40, 3);
        write_u64_le(&mut bytes, dynamic_sh + 48, 8);
        write_u64_le(&mut bytes, dynamic_sh + 56, 16);

        let note_sh = SHDR_OFFSET + SHDR_SIZE * 5;
        write_u32_le(&mut bytes, note_sh, name_offsets[4]);
        write_u32_le(&mut bytes, note_sh + 4, SHT_NOTE);
        write_u64_le(&mut bytes, note_sh + 8, 0x2);
        write_u64_le(&mut bytes, note_sh + 24, NOTE_OFFSET as u64);
        write_u64_le(&mut bytes, note_sh + 32, note_bytes.len() as u64);
        write_u64_le(&mut bytes, note_sh + 48, 8);

        bytes
    }

    fn write_elf(path: &std::path::Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    fn find_struct<'a>(structs: &'a [StructValue], needle: &str) -> Option<&'a StructValue> {
        for sv in structs {
            if sv.name.contains(needle) {
                return Some(sv);
            }
            if let Some(found) = find_struct(&sv.children, needle) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn detects_section_headers_with_names_and_pagination() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sections.elf");
        let extras: Vec<String> = (0..70).map(|idx| format!(".extra_{idx}")).collect();
        let extra_refs: Vec<&str> = extras.iter().map(String::as_str).collect();
        let mut doc = write_elf(&path, &build_elf64_with_sections(&extra_refs));

        let def = detect_with_cap(&mut doc, 3).expect("ELF should be detected");
        let root = &def.structs[0];
        let table = root
            .children
            .iter()
            .find(|child| child.name.starts_with("Section Header Table"))
            .expect("section table");

        let names: Vec<&str> = table
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert!(names.iter().any(|name| name.contains(".shstrtab")));
        assert!(names.iter().any(|name| name.contains(".text")));
        assert!(table
            .children
            .last()
            .unwrap()
            .name
            .contains("use `:insp more` to load more"));
    }

    #[test]
    fn section_data_ranges_point_to_the_actual_section_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("text.elf");
        let mut doc = write_elf(&path, &build_elf64_with_sections(&[]));

        let def = format::detect::detect_format_with_cap(&mut doc, 8).expect("ELF detected");
        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
        let data = find_struct(&structs, "Section Data 2").expect("section data child");
        let field = data
            .fields
            .iter()
            .find(|field| field.def.name == "section_data")
            .expect("section_data field");

        assert_eq!(field.abs_offset, TEXT_OFFSET as u64);
        assert_eq!(field.size, 4);
    }

    #[test]
    fn parses_interpreter_and_dynamic_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dynamic.elf");
        let mut doc = write_elf(&path, &build_elf64_with_dynamic_and_notes());

        let def = format::detect::detect_format_with_cap(&mut doc, 16).expect("ELF detected");
        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");

        let interpreter = find_struct(&structs, "Interpreter").expect("interp");
        assert_eq!(interpreter.fields[0].def.name, "path");
        assert!(interpreter.fields[0]
            .display
            .contains("ld-linux-x86-64.so.2"));

        let needed = find_struct(&structs, "DT_NEEDED -> libc.so.6").expect("needed entry");
        assert_eq!(needed.fields[0].def.name, "d_tag");

        let soname = find_struct(&structs, "DT_SONAME -> sample").expect("soname entry");
        assert!(soname.name.contains("sample"));
    }

    #[test]
    fn parses_gnu_property_notes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.elf");
        let mut doc = write_elf(&path, &build_elf64_with_dynamic_and_notes());

        let def = format::detect::detect_format_with_cap(&mut doc, 16).expect("ELF detected");
        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");

        let note = find_struct(&structs, "NT_GNU_PROPERTY_TYPE_0").expect("gnu property note");
        assert!(note.name.contains("GNU"));

        let property =
            find_struct(&structs, "GNU_PROPERTY_X86_FEATURE_1_AND").expect("gnu property");
        assert!(property.name.contains("Property 0"));
    }
}
