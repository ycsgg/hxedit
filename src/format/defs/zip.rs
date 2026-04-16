use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

/// Local file header signature: PK\x03\x04
const ZIP_LOCAL_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const ZIP_DATA_DESCRIPTOR_FLAG: u16 = 0x0008;

/// Detect and parse ZIP format by scanning Local File Headers from the start.
pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    if doc.len() < 4 {
        return None;
    }

    let magic = read_bytes_raw(doc, 0, 4)?;
    if magic != ZIP_LOCAL_MAGIC {
        return None;
    }

    let mut structs = Vec::new();
    let mut offset: u64 = 0;
    let mut entry_idx = 0;

    while offset + 30 <= doc.len() && entry_idx < 64 {
        let sig = read_bytes_raw(doc, offset, 4)?;
        if sig != ZIP_LOCAL_MAGIC {
            break;
        }

        // Read filename_len and extra_len to compute total header size
        let fname_len_bytes = read_bytes_raw(doc, offset + 26, 2)?;
        let extra_len_bytes = read_bytes_raw(doc, offset + 28, 2)?;
        let fname_len = u16::from_le_bytes([fname_len_bytes[0], fname_len_bytes[1]]) as u64;
        let extra_len = u16::from_le_bytes([extra_len_bytes[0], extra_len_bytes[1]]) as u64;

        let flags_bytes = read_bytes_raw(doc, offset + 6, 2)?;
        let flags = u16::from_le_bytes([flags_bytes[0], flags_bytes[1]]);

        // Read compressed size
        let csize_bytes = read_bytes_raw(doc, offset + 18, 4)?;
        let compressed_size = u32::from_le_bytes([
            csize_bytes[0],
            csize_bytes[1],
            csize_bytes[2],
            csize_bytes[3],
        ]) as u64;

        // Read filename for display
        let fname_display = if fname_len > 0 && fname_len <= 256 {
            read_bytes_raw(doc, offset + 30, fname_len as usize)
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_else(|| format!("entry_{}", entry_idx))
        } else {
            format!("entry_{}", entry_idx)
        };

        let mut fields = vec![
            FieldDef {
                name: "signature".into(),
                offset: 0,
                field_type: FieldType::Bytes(4),
                description: "Local file header signature".into(),
                editable: false,
            },
            FieldDef {
                name: "version_needed".into(),
                offset: 4,
                field_type: FieldType::U16Le,
                description: "Version needed to extract".into(),
                editable: true,
            },
            FieldDef {
                name: "flags".into(),
                offset: 6,
                field_type: FieldType::Flags {
                    inner: Box::new(FieldType::U16Le),
                    flags: vec![
                        (0x0001, "Encrypted".into()),
                        (0x0008, "Data descriptor".into()),
                        (0x0800, "UTF-8".into()),
                    ],
                },
                description: "General purpose bit flag".into(),
                editable: true,
            },
            FieldDef {
                name: "compression".into(),
                offset: 8,
                field_type: FieldType::Enum {
                    inner: Box::new(FieldType::U16Le),
                    variants: vec![(0, "Stored".into()), (8, "Deflated".into())],
                },
                description: "Compression method".into(),
                editable: true,
            },
            FieldDef {
                name: "mod_time".into(),
                offset: 10,
                field_type: FieldType::U16Le,
                description: "Last modification time".into(),
                editable: true,
            },
            FieldDef {
                name: "mod_date".into(),
                offset: 12,
                field_type: FieldType::U16Le,
                description: "Last modification date".into(),
                editable: true,
            },
            FieldDef {
                name: "crc32".into(),
                offset: 14,
                field_type: FieldType::U32Le,
                description: "CRC-32 checksum".into(),
                editable: false,
            },
            FieldDef {
                name: "compressed_size".into(),
                offset: 18,
                field_type: FieldType::U32Le,
                description: "Compressed size".into(),
                editable: true,
            },
            FieldDef {
                name: "uncompressed_size".into(),
                offset: 22,
                field_type: FieldType::U32Le,
                description: "Uncompressed size".into(),
                editable: true,
            },
            FieldDef {
                name: "filename_len".into(),
                offset: 26,
                field_type: FieldType::U16Le,
                description: "Filename length".into(),
                editable: true,
            },
            FieldDef {
                name: "extra_len".into(),
                offset: 28,
                field_type: FieldType::U16Le,
                description: "Extra field length".into(),
                editable: true,
            },
        ];

        if fname_len > 0 && fname_len <= 256 {
            fields.push(FieldDef {
                name: "filename".into(),
                offset: 30,
                field_type: FieldType::Utf8(fname_len as usize),
                description: "Filename".into(),
                editable: false,
            });
        }

        let data_offset = 30 + fname_len + extra_len;
        let has_data_descriptor = flags & ZIP_DATA_DESCRIPTOR_FLAG != 0;
        if !has_data_descriptor && compressed_size > 0 {
            fields.push(FieldDef {
                name: "file_data".into(),
                offset: data_offset,
                field_type: FieldType::DataRange(compressed_size),
                description: "Compressed file data".into(),
                editable: false,
            });
        }

        structs.push(StructDef {
            name: if has_data_descriptor {
                format!(
                    "Local File: {} [data descriptor; partial scan]",
                    fname_display
                )
            } else {
                format!("Local File: {}", fname_display)
            },
            base_offset: offset,
            fields,
            children: vec![],
        });

        if has_data_descriptor {
            break;
        }

        // Advance past header + data
        offset += 30 + fname_len + extra_len + compressed_size;
        entry_idx += 1;
    }

    if structs.is_empty() {
        return None;
    }

    Some(FormatDef {
        name: "ZIP".to_string(),
        structs,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::detect;
    use crate::config::Config;
    use crate::core::document::Document;

    #[test]
    fn stops_local_header_scan_when_data_descriptor_sizes_are_unavailable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("descriptor.zip");

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]); // local header
        bytes.extend_from_slice(&20_u16.to_le_bytes()); // version needed
        bytes.extend_from_slice(&0x0008_u16.to_le_bytes()); // flags: data descriptor
        bytes.extend_from_slice(&0_u16.to_le_bytes()); // compression
        bytes.extend_from_slice(&0_u16.to_le_bytes()); // mod time
        bytes.extend_from_slice(&0_u16.to_le_bytes()); // mod date
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // crc32
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // compressed size unavailable
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // uncompressed size unavailable
        bytes.extend_from_slice(&1_u16.to_le_bytes()); // filename len
        bytes.extend_from_slice(&0_u16.to_le_bytes()); // extra len
        bytes.push(b'a'); // filename
        bytes.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]); // payload starts with local-header magic
        bytes.extend_from_slice(&[0x50, 0x4b, 0x07, 0x08]); // descriptor signature
        bytes.extend_from_slice(&[0; 12]); // descriptor body

        fs::write(&path, bytes).unwrap();

        let mut doc = Document::open(&path, &Config::default()).unwrap();
        let def = detect(&mut doc).expect("zip should still be detected");

        assert_eq!(def.structs.len(), 1);
        assert!(def.structs[0].name.contains("partial scan"));
    }
}
