use super::*;

impl ElfParser<'_> {
    pub(super) fn build_string_table_struct(
        &mut self,
        section: &SectionHeaderInfo,
    ) -> Option<StructDef> {
        if section.size == 0 || section.offset >= self.doc.len() {
            return None;
        }

        let mut entries = Vec::new();
        let mut rel = 0_u64;
        while rel < section.size {
            let Some((text, len)) = self.string_struct(section, rel) else {
                break;
            };
            if len == 0 {
                break;
            }
            entries.push((rel, text, len));
            rel = rel.saturating_add(len as u64);
        }

        let entries: Vec<_> = entries
            .into_iter()
            .filter(|(offset, text, _)| *offset != 0 || !text.is_empty())
            .collect();
        if entries.is_empty() {
            return None;
        }

        let shown = self.shown_count(entries.len());
        let mut children = Vec::new();
        for (rel, text, len) in entries.iter().take(shown) {
            let display = if text.is_empty() {
                "<empty>"
            } else {
                text.as_str()
            };
            children.push(utf8_struct(
                format!("String 0x{rel:x}: {display}"),
                section.offset.saturating_add(*rel),
                "value",
                *len,
                "String table entry",
            ));
        }

        if shown < entries.len() {
            children.push(more_marker(
                format!(
                    "… more strings beyond {} (use `:insp more` to load more)",
                    shown
                ),
                section.offset.saturating_add(entries[shown].0),
            ));
        }

        Some(StructDef {
            name: "String Table Entries".to_owned(),
            base_offset: section.offset,
            fields: vec![],
            children,
        })
    }

    pub(super) fn build_symbol_table_struct(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> Option<StructDef> {
        let layout = self.symbol_layout();
        let entry_size = if section.entsize > 0 {
            section.entsize
        } else {
            layout.entry_size
        };
        if entry_size == 0 || section.size < entry_size || section.offset >= self.doc.len() {
            return None;
        }

        let count = (section.size / entry_size) as usize;
        let shown = self.shown_count(count);
        let word_t = self.word_t();
        let strtab =
            section_link_target(sections, section.link).filter(|s| s.sh_type == SHT_STRTAB);
        let mut children = Vec::new();
        for index in 0..shown {
            let Some(symbol) = self.read_symbol_info(section, sections, index) else {
                break;
            };
            let bind = symbol_bind_label(symbol.info >> 4);
            let sym_type = symbol_type_label(symbol.info & 0x0f);
            let visibility = symbol_visibility_label(symbol.other & 0x03);
            let display_name = if symbol.name.is_empty() {
                "<unnamed>"
            } else {
                symbol.name.as_str()
            };
            let mut symbol_children = Vec::new();
            if let Some(table) = strtab {
                if let Some((text, len)) = self.string_struct(table, symbol.name_offset as u64) {
                    if len > 0 {
                        symbol_children.push(utf8_struct(
                            format!("String: {display_name}"),
                            table.offset.saturating_add(symbol.name_offset as u64),
                            "value",
                            len,
                            "Symbol name string",
                        ));
                        if text.is_empty() {
                            symbol_children.clear();
                        }
                    }
                }
            }

            children.push(StructDef {
                name: format!(
                    "Symbol {}: {} [{}/{}/{}]",
                    index, display_name, sym_type, bind, visibility
                ),
                base_offset: symbol.entry_offset,
                fields: vec![
                    FieldDef {
                        name: "st_name".into(),
                        offset: 0,
                        field_type: self.u32_t(),
                        description: "String-table offset for the symbol name".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "st_info".into(),
                        offset: layout.info_offset,
                        field_type: FieldType::U8,
                        description: "Packed symbol bind/type".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "st_other".into(),
                        offset: layout.other_offset,
                        field_type: FieldType::U8,
                        description: "Packed symbol visibility".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "st_shndx".into(),
                        offset: layout.shndx_offset,
                        field_type: self.u16_t(),
                        description: "Section index".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "st_value".into(),
                        offset: layout.value_offset,
                        field_type: word_t.clone(),
                        description: "Symbol value".into(),
                        editable: false,
                    },
                    FieldDef {
                        name: "st_size".into(),
                        offset: layout.size_offset,
                        field_type: word_t.clone(),
                        description: "Symbol size".into(),
                        editable: false,
                    },
                ],
                children: symbol_children,
            });
        }

        if shown < count {
            children.push(more_marker(
                format!(
                    "… more symbols beyond {} (use `:insp more` to load more)",
                    shown
                ),
                section.offset.saturating_add(shown as u64 * entry_size),
            ));
        }

        Some(StructDef {
            name: "Symbols".to_owned(),
            base_offset: section.offset,
            fields: vec![],
            children,
        })
    }

    pub(super) fn build_relocation_table_struct(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
    ) -> Option<StructDef> {
        let entry_size = if section.entsize > 0 {
            section.entsize
        } else if section.sh_type == SHT_RELA {
            if self.is_64 {
                24
            } else {
                12
            }
        } else if self.is_64 {
            16
        } else {
            8
        };
        if entry_size == 0 || section.size < entry_size || section.offset >= self.doc.len() {
            return None;
        }

        let count = (section.size / entry_size) as usize;
        let shown = self.shown_count(count);
        let word_t = self.word_t();
        let linked_symbols = section_link_target(sections, section.link)
            .filter(|s| matches!(s.sh_type, SHT_SYMTAB | SHT_DYNSYM));
        let machine = self.read_u16(18).unwrap_or(0);
        let addend_t = self.sword_t();
        let mut children = Vec::new();

        for index in 0..shown {
            let entry_offset = section.offset.saturating_add(index as u64 * entry_size);
            let (r_offset, r_info) = if self.is_64 {
                (
                    self.read_u64(entry_offset)?,
                    self.read_u64(entry_offset + 8)?,
                )
            } else {
                (
                    self.read_u32(entry_offset)? as u64,
                    self.read_u32(entry_offset + 4)? as u64,
                )
            };
            let (sym_index, reloc_type) = split_relocation_info(self.is_64, r_info);
            let symbol_name = linked_symbols
                .and_then(|symtab| self.read_symbol_info(symtab, sections, sym_index as usize))
                .map(|symbol| symbol.name)
                .filter(|name| !name.is_empty());
            let reloc_label = relocation_type_label(machine, self.is_64, reloc_type as u32);
            let name = if let Some(symbol_name) = symbol_name {
                format!("Relocation {}: {} -> {}", index, reloc_label, symbol_name)
            } else {
                format!("Relocation {}: {}", index, reloc_label)
            };

            let mut fields = vec![
                FieldDef {
                    name: "r_offset".into(),
                    offset: 0,
                    field_type: word_t.clone(),
                    description: "Location to relocate".into(),
                    editable: false,
                },
                FieldDef {
                    name: "r_info".into(),
                    offset: if self.is_64 { 8 } else { 4 },
                    field_type: word_t.clone(),
                    description: "Packed symbol/type relocation info".into(),
                    editable: false,
                },
            ];
            if section.sh_type == SHT_RELA {
                fields.push(FieldDef {
                    name: "r_addend".into(),
                    offset: if self.is_64 { 16 } else { 8 },
                    field_type: addend_t.clone(),
                    description: "Explicit relocation addend".into(),
                    editable: false,
                });
            }

            let _ = r_offset;
            children.push(StructDef {
                name,
                base_offset: entry_offset,
                fields,
                children: vec![],
            });
        }

        if shown < count {
            children.push(more_marker(
                format!(
                    "… more relocations beyond {} (use `:insp more` to load more)",
                    shown
                ),
                section.offset.saturating_add(shown as u64 * entry_size),
            ));
        }

        Some(StructDef {
            name: "Relocations".to_owned(),
            base_offset: section.offset,
            fields: vec![],
            children,
        })
    }

    pub(super) fn build_sysv_hash_struct(
        &mut self,
        section: &SectionHeaderInfo,
    ) -> Option<StructDef> {
        let base = section.offset;
        let nbucket = self.read_u32(base)?;
        let nchain = self.read_u32(base + 4)?;
        let buckets_offset = base + 8;
        let chains_offset = buckets_offset + nbucket as u64 * 4;

        Some(StructDef {
            name: "SysV Hash".to_owned(),
            base_offset: base,
            fields: vec![
                FieldDef {
                    name: "nbucket".into(),
                    offset: 0,
                    field_type: self.u32_t(),
                    description: "Number of hash buckets".into(),
                    editable: false,
                },
                FieldDef {
                    name: "nchain".into(),
                    offset: 4,
                    field_type: self.u32_t(),
                    description: "Number of chain entries".into(),
                    editable: false,
                },
            ],
            children: vec![
                data_range_struct(
                    "Buckets".to_owned(),
                    buckets_offset,
                    "buckets",
                    nbucket as u64 * 4,
                    "SysV hash buckets",
                ),
                data_range_struct(
                    "Chains".to_owned(),
                    chains_offset,
                    "chains",
                    nchain as u64 * 4,
                    "SysV hash chains",
                ),
            ],
        })
    }

    pub(super) fn build_gnu_hash_struct(
        &mut self,
        section: &SectionHeaderInfo,
    ) -> Option<StructDef> {
        let base = section.offset;
        let nbuckets = self.read_u32(base)?;
        let _symoffset = self.read_u32(base + 4)?;
        let bloom_size = self.read_u32(base + 8)?;
        let _bloom_shift = self.read_u32(base + 12)?;
        let bloom_word_size = if self.is_64 { 8 } else { 4 };
        let bloom_offset = base + 16;
        let buckets_offset = bloom_offset + bloom_size as u64 * bloom_word_size;
        let chains_offset = buckets_offset + nbuckets as u64 * 4;
        let chains_size = section
            .size
            .saturating_sub(chains_offset.saturating_sub(base))
            .min(section.size);

        Some(StructDef {
            name: "GNU Hash".to_owned(),
            base_offset: base,
            fields: vec![
                FieldDef {
                    name: "nbuckets".into(),
                    offset: 0,
                    field_type: self.u32_t(),
                    description: "Number of GNU hash buckets".into(),
                    editable: false,
                },
                FieldDef {
                    name: "symoffset".into(),
                    offset: 4,
                    field_type: self.u32_t(),
                    description: "First symbol index covered by the hash".into(),
                    editable: false,
                },
                FieldDef {
                    name: "bloom_size".into(),
                    offset: 8,
                    field_type: self.u32_t(),
                    description: "Bloom filter word count".into(),
                    editable: false,
                },
                FieldDef {
                    name: "bloom_shift".into(),
                    offset: 12,
                    field_type: self.u32_t(),
                    description: "Bloom filter shift".into(),
                    editable: false,
                },
            ],
            children: vec![
                data_range_struct(
                    "Bloom Filter".to_owned(),
                    bloom_offset,
                    "bloom",
                    bloom_size as u64 * bloom_word_size,
                    "GNU hash bloom filter",
                ),
                data_range_struct(
                    "Buckets".to_owned(),
                    buckets_offset,
                    "buckets",
                    nbuckets as u64 * 4,
                    "GNU hash buckets",
                ),
                data_range_struct(
                    "Chains".to_owned(),
                    chains_offset,
                    "chains",
                    chains_size,
                    "GNU hash chain table",
                ),
            ],
        })
    }

    pub(super) fn read_symbol_info(
        &mut self,
        section: &SectionHeaderInfo,
        sections: &[SectionHeaderInfo],
        index: usize,
    ) -> Option<SymbolInfo> {
        let layout = self.symbol_layout();
        let entry_size = if section.entsize > 0 {
            section.entsize
        } else {
            layout.entry_size
        };
        if entry_size == 0 {
            return None;
        }
        let rel = index as u64 * entry_size;
        if rel.saturating_add(entry_size) > section.size {
            return None;
        }
        let entry_offset = section.offset.saturating_add(rel);
        self.require_bytes(entry_offset, layout.entry_size)?;
        let name_offset = self.read_u32(entry_offset)?;
        let info = read_u8(self.doc, entry_offset + layout.info_offset)?;
        let other = read_u8(self.doc, entry_offset + layout.other_offset)?;
        let name = section_link_target(sections, section.link)
            .filter(|table| table.sh_type == SHT_STRTAB)
            .and_then(|table| self.read_string_from_table(table, name_offset as u64))
            .unwrap_or_default();
        Some(SymbolInfo {
            entry_offset,
            name_offset,
            name,
            info,
            other,
        })
    }
}
