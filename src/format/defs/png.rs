use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

/// PNG signature: 89 50 4e 47 0d 0a 1a 0a
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// Detect and parse PNG format.
pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    if doc.len() < 8 {
        return None;
    }

    let magic = read_bytes_raw(doc, 0, 8)?;
    if magic != PNG_MAGIC {
        return None;
    }

    let mut structs = vec![StructDef {
        name: "PNG Signature".into(),
        base_offset: 0,
        fields: vec![FieldDef {
            name: "signature".into(),
            offset: 0,
            field_type: FieldType::Bytes(8),
            description: "PNG file signature".into(),
            editable: false,
        }],
        children: vec![],
    }];

    // Iterate chunks
    let mut offset: u64 = 8;
    let mut chunk_idx = 0;
    while offset + 12 <= doc.len() && chunk_idx < 64 {
        let len_bytes = read_bytes_raw(doc, offset, 4)?;
        let chunk_len =
            u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as u64;
        let type_bytes = read_bytes_raw(doc, offset + 4, 4)?;
        let chunk_type = String::from_utf8_lossy(&type_bytes).to_string();

        // Reject chunks whose declared length would run past EOF. Without this
        // check a malformed PNG keeps scrolling `offset` forward into garbage
        // until the outer loop happens to bail out, which looks like a TUI
        // freeze instead of a parse error.
        let chunk_end = offset
            .checked_add(12)
            .and_then(|o| o.checked_add(chunk_len));
        let truncated = match chunk_end {
            Some(end) => end > doc.len(),
            None => true,
        };

        let mut fields = vec![
            FieldDef {
                name: "length".into(),
                offset: 0,
                field_type: FieldType::U32Be,
                description: "Chunk data length".into(),
                editable: true,
            },
            FieldDef {
                name: "type".into(),
                offset: 4,
                field_type: FieldType::Utf8(4),
                description: "Chunk type".into(),
                editable: false,
            },
        ];

        // Parse IHDR fields
        if chunk_type == "IHDR" && chunk_len >= 13 && !truncated {
            fields.extend(vec![
                FieldDef {
                    name: "width".into(),
                    offset: 8,
                    field_type: FieldType::U32Be,
                    description: "Image width".into(),
                    editable: true,
                },
                FieldDef {
                    name: "height".into(),
                    offset: 12,
                    field_type: FieldType::U32Be,
                    description: "Image height".into(),
                    editable: true,
                },
                FieldDef {
                    name: "bit_depth".into(),
                    offset: 16,
                    field_type: FieldType::U8,
                    description: "Bit depth".into(),
                    editable: true,
                },
                FieldDef {
                    name: "color_type".into(),
                    offset: 17,
                    field_type: FieldType::Enum {
                        inner: Box::new(FieldType::U8),
                        variants: vec![
                            (0, "Grayscale".into()),
                            (2, "RGB".into()),
                            (3, "Indexed".into()),
                            (4, "Grayscale+Alpha".into()),
                            (6, "RGBA".into()),
                        ],
                    },
                    description: "Color type".into(),
                    editable: true,
                },
                FieldDef {
                    name: "compression".into(),
                    offset: 18,
                    field_type: FieldType::U8,
                    description: "Compression method".into(),
                    editable: true,
                },
                FieldDef {
                    name: "filter".into(),
                    offset: 19,
                    field_type: FieldType::U8,
                    description: "Filter method".into(),
                    editable: true,
                },
                FieldDef {
                    name: "interlace".into(),
                    offset: 20,
                    field_type: FieldType::Enum {
                        inner: Box::new(FieldType::U8),
                        variants: vec![(0, "None".into()), (1, "Adam7".into())],
                    },
                    description: "Interlace method".into(),
                    editable: true,
                },
            ]);
        }

        // CRC + data fields only make sense when the chunk actually fits in
        // the file. For a truncated header we leave the rest out so the UI
        // doesn't pretend those bytes exist.
        if !truncated {
            fields.push(FieldDef {
                name: "crc".into(),
                offset: 8 + chunk_len,
                field_type: FieldType::U32Be,
                description: "CRC-32 checksum".into(),
                editable: false,
            });

            if chunk_len > 0 {
                fields.push(FieldDef {
                    name: "data".into(),
                    offset: 8,
                    field_type: FieldType::DataRange(chunk_len),
                    description: "Chunk data".into(),
                    editable: false,
                });
            }
        }

        structs.push(StructDef {
            name: if truncated {
                format!("Chunk: {} (truncated)", chunk_type)
            } else {
                format!("Chunk: {}", chunk_type)
            },
            base_offset: offset,
            fields,
            children: vec![],
        });

        if truncated || chunk_type == "IEND" {
            break;
        }

        offset += 12 + chunk_len;
        chunk_idx += 1;
    }

    Some(FormatDef {
        name: "PNG".to_string(),
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

    fn write_png(path: &std::path::Path, bytes: Vec<u8>) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    #[test]
    fn marks_chunk_truncated_when_declared_length_overflows_eof() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.png");

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&super::PNG_MAGIC);
        // Chunk declares 1 GiB of data but only 12 bytes of header + 4 bytes
        // of (missing) crc slot follow. We still need `offset + 12 <=
        // doc.len()` to enter the loop, so pad out enough to read length +
        // type but not the declared data/crc.
        bytes.extend_from_slice(&(0x4000_0000_u32).to_be_bytes()); // length
        bytes.extend_from_slice(b"IHDR"); // type
        bytes.extend_from_slice(&[0u8; 4]); // pad so loop body runs

        let mut doc = write_png(&path, bytes);
        let def = detect(&mut doc).expect("detect should still succeed for valid magic");
        assert!(def.structs.len() >= 2);
        let chunk_header_name = &def.structs[1].name;
        assert!(
            chunk_header_name.contains("(truncated)"),
            "expected truncated marker, got {}",
            chunk_header_name
        );

        // A truncated chunk must not advertise a data or crc field pointing
        // past EOF.
        assert!(
            !def.structs[1]
                .fields
                .iter()
                .any(|f| f.name == "data" || f.name == "crc"),
            "truncated chunk should not expose data/crc fields"
        );
    }

    #[test]
    fn stops_after_truncated_chunk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("trailing.png");

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&super::PNG_MAGIC);
        // Truncated chunk first.
        bytes.extend_from_slice(&(0xffff_u32).to_be_bytes());
        bytes.extend_from_slice(b"junk");
        // Pad to satisfy the `offset + 12 <= doc.len()` guard and give the
        // loop something to (incorrectly) advance into if the truncation
        // check were missing.
        bytes.extend_from_slice(&[0; 64]);

        let mut doc = write_png(&path, bytes);
        let def = detect(&mut doc).expect("detect succeeds");
        // Expect only signature + the one truncated chunk.
        assert_eq!(def.structs.len(), 2);
    }
}
