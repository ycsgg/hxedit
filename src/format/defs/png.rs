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
        if chunk_type == "IHDR" && chunk_len >= 13 {
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

        // CRC field at end of chunk
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

        structs.push(StructDef {
            name: format!("Chunk: {}", chunk_type),
            base_offset: offset,
            fields,
            children: vec![],
        });

        if chunk_type == "IEND" {
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
