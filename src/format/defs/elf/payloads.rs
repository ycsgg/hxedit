use super::*;

impl ElfParser<'_> {
    pub(super) fn build_segment_payload_children(
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

    pub(super) fn build_section_payload_children(
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

        if section.sh_type == SHT_STRTAB {
            if let Some(strings) = self.build_string_table_struct(section) {
                children.push(strings);
            }
        }

        if matches!(section.sh_type, SHT_SYMTAB | SHT_DYNSYM) {
            if let Some(symbols) = self.build_symbol_table_struct(section, sections) {
                children.push(symbols);
            }
        }

        if matches!(section.sh_type, SHT_REL | SHT_RELA) {
            if let Some(relocations) = self.build_relocation_table_struct(section, sections) {
                children.push(relocations);
            }
        }

        if section.sh_type == SHT_HASH {
            if let Some(hash) = self.build_sysv_hash_struct(section) {
                children.push(hash);
            }
        }

        if section.sh_type == SHT_GNU_HASH {
            if let Some(hash) = self.build_gnu_hash_struct(section) {
                children.push(hash);
            }
        }

        if section.sh_type == SHT_GNU_VERNEED {
            if let Some(verneed) = self.build_verneed_struct(section, sections) {
                children.push(verneed);
            }
        }

        if section.sh_type == SHT_GNU_VERDEF {
            if let Some(verdef) = self.build_verdef_struct(section, sections) {
                children.push(verdef);
            }
        }

        if section.sh_type == SHT_GNU_VERSYM {
            if let Some(versym) = self.build_versym_struct(section, sections) {
                children.push(versym);
            }
        }

        children
    }

    pub(super) fn build_interpreter_struct(
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

    pub(super) fn build_dynamic_entries_struct(
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

    pub(super) fn build_notes_struct(
        &mut self,
        name: String,
        offset: u64,
        size: u64,
    ) -> Option<StructDef> {
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

    pub(super) fn build_gnu_properties_struct(
        &mut self,
        offset: u64,
        size: u64,
    ) -> Option<StructDef> {
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

    pub(super) fn string_struct(
        &mut self,
        table: &SectionHeaderInfo,
        offset: u64,
    ) -> Option<(String, usize)> {
        if table.sh_type != SHT_STRTAB || offset >= table.size {
            return None;
        }
        let abs = table.offset.checked_add(offset)?;
        self.read_c_string(abs, table.size.saturating_sub(offset))
    }

    pub(super) fn read_string_from_table(
        &mut self,
        table: &SectionHeaderInfo,
        offset: u64,
    ) -> Option<String> {
        if table.sh_type != SHT_STRTAB || offset >= table.size {
            return None;
        }
        let abs = table.offset.checked_add(offset)?;
        let remaining = table.size.saturating_sub(offset);
        self.read_c_string(abs, remaining).map(|(text, _)| text)
    }

    pub(super) fn read_c_string(&mut self, offset: u64, max_len: u64) -> Option<(String, usize)> {
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
}
