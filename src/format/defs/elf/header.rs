use super::*;

impl ElfParser<'_> {
    pub(super) fn build_header_fields(
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

    pub(super) fn parse_program_headers(
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

    pub(super) fn parse_section_headers(
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
}
