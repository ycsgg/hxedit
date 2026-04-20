use super::*;

impl ElfParser<'_> {
    pub(super) fn build_verneed_struct(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> Option<StructDef> {
        let strtab =
            section_link_target(sections, section.link).filter(|s| s.sh_type == SHT_STRTAB)?;
        let end = section.offset.saturating_add(section.size);
        let mut children = Vec::new();
        let mut cursor = section.offset;
        let mut count = 0_usize;

        while cursor.saturating_add(16) <= end && count < self.entry_cap.max(1) {
            let _vn_version = self.read_u16(cursor)?;
            let vn_cnt = self.read_u16(cursor + 2)?;
            let vn_file = self.read_u32(cursor + 4)?;
            let vn_aux = self.read_u32(cursor + 8)?;
            let vn_next = self.read_u32(cursor + 12)?;
            let file_name = self
                .read_string_from_table(strtab, vn_file as u64)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "<unnamed>".to_owned());

            let mut aux_children = Vec::new();
            let mut aux_cursor = cursor.saturating_add(vn_aux as u64);
            let aux_limit = vn_cnt.min(self.entry_cap.max(1) as u16) as usize;
            for aux_index in 0..aux_limit {
                if aux_cursor.saturating_add(16) > end {
                    break;
                }
                let _vna_hash = self.read_u32(aux_cursor)?;
                let _vna_flags = self.read_u16(aux_cursor + 4)?;
                let vna_other = self.read_u16(aux_cursor + 6)?;
                let vna_name = self.read_u32(aux_cursor + 8)?;
                let vna_next = self.read_u32(aux_cursor + 12)?;
                let name = self
                    .read_string_from_table(strtab, vna_name as u64)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "<unnamed>".to_owned());
                aux_children.push(StructDef {
                    name: format!("Vernaux {}: {} [index {}]", aux_index, name, vna_other),
                    base_offset: aux_cursor,
                    fields: vec![
                        FieldDef {
                            name: "vna_hash".into(),
                            offset: 0,
                            field_type: self.u32_t(),
                            description: "Version hash".into(),
                            editable: false,
                        },
                        FieldDef {
                            name: "vna_flags".into(),
                            offset: 4,
                            field_type: self.u16_t(),
                            description: "Version flags".into(),
                            editable: false,
                        },
                        FieldDef {
                            name: "vna_other".into(),
                            offset: 6,
                            field_type: self.u16_t(),
                            description: "Version index".into(),
                            editable: false,
                        },
                        FieldDef {
                            name: "vna_name".into(),
                            offset: 8,
                            field_type: self.u32_t(),
                            description: "String-table offset for the version name".into(),
                            editable: false,
                        },
                        FieldDef {
                            name: "vna_next".into(),
                            offset: 12,
                            field_type: self.u32_t(),
                            description: "Offset to the next auxiliary entry".into(),
                            editable: false,
                        },
                    ],
                    children: vec![],
                });
                if vna_next == 0 {
                    break;
                }
                aux_cursor = aux_cursor.saturating_add(vna_next as u64);
            }

            children.push(StructDef {
                name: format!("Verneed {}: {}", count, file_name),
                base_offset: cursor,
                fields: vec![
                    FieldDef {
                        name: "vn_version".into(),
                        offset: 0,
                        field_type: self.u16_t(),
                        description: "Version need structure version".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vn_cnt".into(),
                        offset: 2,
                        field_type: self.u16_t(),
                        description: "Number of auxiliary entries".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vn_file".into(),
                        offset: 4,
                        field_type: self.u32_t(),
                        description: "String-table offset for the dependency filename".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vn_aux".into(),
                        offset: 8,
                        field_type: self.u32_t(),
                        description: "Offset to the first auxiliary entry".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vn_next".into(),
                        offset: 12,
                        field_type: self.u32_t(),
                        description: "Offset to the next version need entry".into(),
                        editable: false,
                    },
                ],
                children: aux_children,
            });
            count += 1;
            if vn_next == 0 {
                break;
            }
            cursor = cursor.saturating_add(vn_next as u64);
        }

        if children.is_empty() {
            None
        } else {
            Some(StructDef {
                name: "Version Needs".to_owned(),
                base_offset: section.offset,
                fields: vec![],
                children,
            })
        }
    }

    pub(super) fn build_verdef_struct(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> Option<StructDef> {
        let strtab =
            section_link_target(sections, section.link).filter(|s| s.sh_type == SHT_STRTAB)?;
        let end = section.offset.saturating_add(section.size);
        let mut children = Vec::new();
        let mut cursor = section.offset;
        let mut count = 0_usize;

        while cursor.saturating_add(20) <= end && count < self.entry_cap.max(1) {
            let _vd_version = self.read_u16(cursor)?;
            let _vd_flags = self.read_u16(cursor + 2)?;
            let vd_ndx = self.read_u16(cursor + 4)?;
            let vd_cnt = self.read_u16(cursor + 6)?;
            let _vd_hash = self.read_u32(cursor + 8)?;
            let vd_aux = self.read_u32(cursor + 12)?;
            let vd_next = self.read_u32(cursor + 16)?;

            let mut aux_children = Vec::new();
            let aux_cursor = cursor.saturating_add(vd_aux as u64);
            if aux_cursor.saturating_add(8) <= end {
                let vda_name = self.read_u32(aux_cursor)?;
                let vda_next = self.read_u32(aux_cursor + 4)?;
                let name = self
                    .read_string_from_table(strtab, vda_name as u64)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "<unnamed>".to_owned());
                let _ = vd_cnt;
                aux_children.push(StructDef {
                    name: format!("Verdaux: {}", name),
                    base_offset: aux_cursor,
                    fields: vec![
                        FieldDef {
                            name: "vda_name".into(),
                            offset: 0,
                            field_type: self.u32_t(),
                            description: "String-table offset for the version definition name"
                                .into(),
                            editable: false,
                        },
                        FieldDef {
                            name: "vda_next".into(),
                            offset: 4,
                            field_type: self.u32_t(),
                            description: "Offset to the next auxiliary definition".into(),
                            editable: false,
                        },
                    ],
                    children: vec![],
                });
                let _ = vda_next;
            }

            children.push(StructDef {
                name: format!("Verdef {}: index {}", count, vd_ndx),
                base_offset: cursor,
                fields: vec![
                    FieldDef {
                        name: "vd_version".into(),
                        offset: 0,
                        field_type: self.u16_t(),
                        description: "Version definition structure version".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vd_flags".into(),
                        offset: 2,
                        field_type: self.u16_t(),
                        description: "Version definition flags".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vd_ndx".into(),
                        offset: 4,
                        field_type: self.u16_t(),
                        description: "Version definition index".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vd_cnt".into(),
                        offset: 6,
                        field_type: self.u16_t(),
                        description: "Number of auxiliary definition entries".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vd_hash".into(),
                        offset: 8,
                        field_type: self.u32_t(),
                        description: "Version name hash".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vd_aux".into(),
                        offset: 12,
                        field_type: self.u32_t(),
                        description: "Offset to the first auxiliary definition entry".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "vd_next".into(),
                        offset: 16,
                        field_type: self.u32_t(),
                        description: "Offset to the next version definition".into(),
                        editable: false,
                    },
                ],
                children: aux_children,
            });
            count += 1;
            if vd_next == 0 {
                break;
            }
            cursor = cursor.saturating_add(vd_next as u64);
        }

        if children.is_empty() {
            None
        } else {
            Some(StructDef {
                name: "Version Definitions".to_owned(),
                base_offset: section.offset,
                fields: vec![],
                children,
            })
        }
    }

    pub(super) fn build_versym_struct(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> Option<StructDef> {
        if section.size < 2 || section.offset >= self.doc.len() {
            return None;
        }
        let symbol_section = section_link_target(sections, section.link)
            .filter(|s| matches!(s.sh_type, SHT_SYMTAB | SHT_DYNSYM));
        let version_map = self.version_name_map(sections);
        let count = (section.size / 2) as usize;
        let shown = self.shown_count(count);
        let mut children = Vec::new();
        for index in 0..shown {
            let entry_offset = section.offset.saturating_add(index as u64 * 2);
            let raw = self.read_u16(entry_offset)?;
            let hidden = raw & 0x8000 != 0;
            let version_index = raw & 0x7fff;
            let symbol_name = symbol_section
                .and_then(|symtab| self.read_symbol_info(symtab, sections, index))
                .map(|symbol| symbol.name)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("#{}", index));
            let version_name = version_map
                .get(&version_index)
                .cloned()
                .unwrap_or_else(|| format!("index {}", version_index));
            children.push(StructDef {
                name: format!(
                    "Versym {}: {} -> {}{}",
                    index,
                    symbol_name,
                    version_name,
                    if hidden { " [hidden]" } else { "" }
                ),
                base_offset: entry_offset,
                fields: vec![FieldDef {
                    name: "vs_index".into(),
                    offset: 0,
                    field_type: self.u16_t(),
                    description: "Version symbol index".into(),
                    editable: false,
                }],
                children: vec![],
            });
        }

        if shown < count {
            children.push(more_marker(
                format!(
                    "… more version symbols beyond {} (use `:insp more` to load more)",
                    shown
                ),
                section.offset.saturating_add(shown as u64 * 2),
            ));
        }

        Some(StructDef {
            name: "Version Symbols".to_owned(),
            base_offset: section.offset,
            fields: vec![],
            children,
        })
    }

    pub(super) fn version_name_map(
        &mut self,
        sections: &[SectionHeaderInfo],
    ) -> std::collections::BTreeMap<u16, String> {
        let mut map = std::collections::BTreeMap::new();

        for section in sections
            .iter()
            .filter(|section| section.sh_type == SHT_GNU_VERDEF)
        {
            let Some(strtab) =
                section_link_target(sections, section.link).filter(|s| s.sh_type == SHT_STRTAB)
            else {
                continue;
            };
            let end = section.offset.saturating_add(section.size);
            let mut cursor = section.offset;
            while cursor.saturating_add(20) <= end {
                let Some(vd_ndx) = self.read_u16(cursor + 4) else {
                    break;
                };
                let Some(vd_aux) = self.read_u32(cursor + 12) else {
                    break;
                };
                let Some(vd_next) = self.read_u32(cursor + 16) else {
                    break;
                };
                let aux_cursor = cursor.saturating_add(vd_aux as u64);
                if aux_cursor.saturating_add(4) <= end {
                    if let Some(name_offset) = self.read_u32(aux_cursor) {
                        if let Some(name) = self.read_string_from_table(strtab, name_offset as u64)
                        {
                            if !name.is_empty() {
                                map.entry(vd_ndx).or_insert(name);
                            }
                        }
                    }
                }
                if vd_next == 0 {
                    break;
                }
                cursor = cursor.saturating_add(vd_next as u64);
            }
        }

        for section in sections
            .iter()
            .filter(|section| section.sh_type == SHT_GNU_VERNEED)
        {
            let Some(strtab) =
                section_link_target(sections, section.link).filter(|s| s.sh_type == SHT_STRTAB)
            else {
                continue;
            };
            let end = section.offset.saturating_add(section.size);
            let mut cursor = section.offset;
            while cursor.saturating_add(16) <= end {
                let Some(vn_cnt) = self.read_u16(cursor + 2) else {
                    break;
                };
                let Some(vn_aux) = self.read_u32(cursor + 8) else {
                    break;
                };
                let Some(vn_next) = self.read_u32(cursor + 12) else {
                    break;
                };
                let mut aux_cursor = cursor.saturating_add(vn_aux as u64);
                for _ in 0..vn_cnt {
                    if aux_cursor.saturating_add(16) > end {
                        break;
                    }
                    let Some(vna_other) = self.read_u16(aux_cursor + 6) else {
                        break;
                    };
                    let Some(vna_name) = self.read_u32(aux_cursor + 8) else {
                        break;
                    };
                    let Some(vna_next) = self.read_u32(aux_cursor + 12) else {
                        break;
                    };
                    if let Some(name) = self.read_string_from_table(strtab, vna_name as u64) {
                        if !name.is_empty() {
                            map.entry(vna_other).or_insert(name);
                        }
                    }
                    if vna_next == 0 {
                        break;
                    }
                    aux_cursor = aux_cursor.saturating_add(vna_next as u64);
                }
                if vn_next == 0 {
                    break;
                }
                cursor = cursor.saturating_add(vn_next as u64);
            }
        }

        map
    }
}
