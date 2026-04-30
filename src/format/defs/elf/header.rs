use super::*;

impl ElfParser<'_> {
    pub(super) fn build_header_fields(&mut self) -> Option<HeaderSummary> {
        let header_layout = self.header_layout();
        if self.doc.len() < header_layout.ehdr_size {
            return None;
        }
        let u16_t = self.u16_t();
        let u32_t = self.u32_t();
        let addr_t = self.word_t();
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
                offset: header_layout.phoff_offset,
                field_type: off_t.clone(),
                description: "Program header table offset".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shoff".into(),
                offset: header_layout.shoff_offset,
                field_type: off_t.clone(),
                description: "Section header table offset".into(),
                editable: true,
            },
            FieldDef {
                name: "e_flags".into(),
                offset: header_layout.flags_offset,
                field_type: u32_t.clone(),
                description: "Processor-specific flags".into(),
                editable: true,
            },
            FieldDef {
                name: "e_ehsize".into(),
                offset: header_layout.ehsize_offset,
                field_type: u16_t.clone(),
                description: "ELF header size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phentsize".into(),
                offset: header_layout.phentsize_offset,
                field_type: u16_t.clone(),
                description: "Program header entry size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phnum".into(),
                offset: header_layout.phnum_offset,
                field_type: u16_t.clone(),
                description: "Number of program headers".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shentsize".into(),
                offset: header_layout.shentsize_offset,
                field_type: u16_t.clone(),
                description: "Section header entry size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shnum".into(),
                offset: header_layout.shnum_offset,
                field_type: u16_t.clone(),
                description: "Number of section headers".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shstrndx".into(),
                offset: header_layout.shstrndx_offset,
                field_type: u16_t.clone(),
                description: "Section name string table index".into(),
                editable: true,
            },
        ]);

        let phoff = self.read_word(header_layout.phoff_offset)?;
        let phnum = usize::from(self.read_u16(header_layout.phnum_offset)?);
        let phentsize = u64::from(self.read_u16(header_layout.phentsize_offset)?);
        let shoff = self.read_word(header_layout.shoff_offset)?;
        let shnum = usize::from(self.read_u16(header_layout.shnum_offset)?);
        let shentsize = u64::from(self.read_u16(header_layout.shentsize_offset)?);
        let shstrndx = usize::from(self.read_u16(header_layout.shstrndx_offset)?);

        Some(HeaderSummary {
            fields,
            phoff,
            phnum,
            phentsize,
            shoff,
            shnum,
            shentsize,
            shstrndx,
        })
    }

    pub(super) fn parse_program_headers(
        &mut self,
        phoff: u64,
        phnum: usize,
        phentsize: u64,
    ) -> Vec<ProgramHeaderInfo> {
        let layout = self.program_header_layout();
        if phoff == 0 || phnum == 0 || phentsize < layout.entry_size {
            return Vec::new();
        }

        let mut out = Vec::new();
        for index in 0..phnum {
            let base = phoff.saturating_add(index as u64 * phentsize);
            if self.require_bytes(base, layout.entry_size).is_none() {
                break;
            }
            let Some(p_type) = self.read_u32(base) else {
                break;
            };
            let Some(offset) = self.read_word(base + layout.offset_offset) else {
                break;
            };
            let Some(filesz) = self.read_word(base + layout.filesz_offset) else {
                break;
            };
            out.push(ProgramHeaderInfo {
                index,
                entry_offset: base,
                p_type,
                offset,
                filesz,
            });
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
        let layout = self.section_header_layout();
        if shoff == 0 || shnum == 0 || shentsize < layout.entry_size {
            return Vec::new();
        }

        let mut out = Vec::new();
        for index in 0..shnum {
            let base = shoff.saturating_add(index as u64 * shentsize);
            if self.require_bytes(base, layout.entry_size).is_none() {
                break;
            }
            let Some(name_offset) = self.read_u32(base) else {
                break;
            };
            let Some(sh_type) = self.read_u32(base + 4) else {
                break;
            };
            let Some(offset) = self.read_word(base + layout.offset_offset) else {
                break;
            };
            let Some(size) = self.read_word(base + layout.size_offset) else {
                break;
            };
            let Some(link) = self.read_u32(base + layout.link_offset) else {
                break;
            };
            let Some(entsize) = self.read_word(base + layout.entsize_offset) else {
                break;
            };
            out.push(SectionHeaderInfo {
                index,
                entry_offset: base,
                name_offset,
                name: String::new(),
                sh_type,
                offset,
                size,
                link,
                entsize,
            });
        }

        let shstrtab = out
            .get(shstrndx)
            .filter(|table| table.sh_type == SHT_STRTAB)
            .map(|table| (table.offset, table.size));
        for section in &mut out {
            section.name = shstrtab
                .and_then(|(offset, size)| {
                    self.read_string_from_table_range(offset, size, section.name_offset as u64)
                })
                .unwrap_or_default();
        }

        out
    }
}
