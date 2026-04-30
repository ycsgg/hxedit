use super::*;

impl ElfParser<'_> {
    pub(super) fn build_program_header_table(
        &mut self,
        phoff: u64,
        total_count: usize,
        headers: &[ProgramHeaderInfo],
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let layout = self.program_header_layout();
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
                phoff.saturating_add(shown as u64 * layout.entry_size),
            ));
        }

        StructDef {
            name: format!("Program Header Table ({} entries)", total_count),
            base_offset: phoff,
            fields: vec![],
            children,
        }
    }

    pub(super) fn build_section_header_table(
        &mut self,
        shoff: u64,
        total_count: usize,
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let layout = self.section_header_layout();
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
                shoff.saturating_add(shown as u64 * layout.entry_size),
            ));
        }

        StructDef {
            name: format!("Section Header Table ({} entries)", total_count),
            base_offset: shoff,
            fields: vec![],
            children,
        }
    }

    pub(super) fn build_program_header_struct(
        &mut self,
        header: &ProgramHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let layout = self.program_header_layout();
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
                    offset: layout.flags_offset,
                    field_type: FieldType::Flags {
                        inner: Box::new(u32_t.clone()),
                        flags: vec![(4, "R".into()), (2, "W".into()), (1, "X".into())],
                    },
                    description: "Segment flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_offset".into(),
                    offset: layout.offset_offset,
                    field_type: addr_t.clone(),
                    description: "File offset of segment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_vaddr".into(),
                    offset: layout.vaddr_offset,
                    field_type: addr_t.clone(),
                    description: "Virtual address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_paddr".into(),
                    offset: layout.paddr_offset,
                    field_type: addr_t.clone(),
                    description: "Physical address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_filesz".into(),
                    offset: layout.filesz_offset,
                    field_type: addr_t.clone(),
                    description: "Size in file".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_memsz".into(),
                    offset: layout.memsz_offset,
                    field_type: addr_t.clone(),
                    description: "Size in memory".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_align".into(),
                    offset: layout.align_offset,
                    field_type: addr_t,
                    description: "Alignment".into(),
                    editable: true,
                },
            ],
            children,
        }
    }

    pub(super) fn build_section_header_struct(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> StructDef {
        let layout = self.section_header_layout();
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
                    offset: layout.flags_offset,
                    field_type: FieldType::Flags {
                        inner: Box::new(word_t.clone()),
                        flags: section_flag_variants(),
                    },
                    description: "Section attribute flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_addr".into(),
                    offset: layout.addr_offset,
                    field_type: word_t.clone(),
                    description: "Virtual address in memory".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_offset".into(),
                    offset: layout.offset_offset,
                    field_type: word_t.clone(),
                    description: "File offset of section data".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_size".into(),
                    offset: layout.size_offset,
                    field_type: word_t.clone(),
                    description: "Size of section data".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_link".into(),
                    offset: layout.link_offset,
                    field_type: u32_t.clone(),
                    description: "Section-specific link".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_info".into(),
                    offset: layout.info_offset,
                    field_type: u32_t.clone(),
                    description: "Section-specific extra info".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_addralign".into(),
                    offset: layout.addralign_offset,
                    field_type: word_t.clone(),
                    description: "Required alignment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "sh_entsize".into(),
                    offset: layout.entsize_offset,
                    field_type: word_t,
                    description: "Entry size for table sections".into(),
                    editable: true,
                },
            ],
            children,
        }
    }
}
