use crate::core::document::Document;
use crate::format::detect::{read_bytes_raw, read_u8};
use crate::format::types::*;

const GIF_HEADER_LEN: u64 = 6;
const LOGICAL_SCREEN_DESCRIPTOR_LEN: u64 = 7;
const IMAGE_DESCRIPTOR_LEN: u64 = 10;
const GRAPHIC_CONTROL_DATA_LEN: u64 = 4;
const APPLICATION_IDENTIFIER_LEN: usize = 8;
const APPLICATION_AUTH_CODE_LEN: usize = 3;
const PLAINTEXT_HEADER_LEN: u64 = 12;

const EXTENSION_INTRODUCER: u8 = 0x21;
const IMAGE_SEPARATOR: u8 = 0x2c;
const TRAILER: u8 = 0x3b;

const GRAPHIC_CONTROL_LABEL: u8 = 0xf9;
const COMMENT_LABEL: u8 = 0xfe;
const PLAINTEXT_LABEL: u8 = 0x01;
const APPLICATION_LABEL: u8 = 0xff;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < GIF_HEADER_LEN + LOGICAL_SCREEN_DESCRIPTOR_LEN {
        return None;
    }

    let header = read_bytes_raw(
        doc,
        0,
        (GIF_HEADER_LEN + LOGICAL_SCREEN_DESCRIPTOR_LEN) as usize,
    )?;
    if &header[0..3] != b"GIF" {
        return None;
    }
    let version = &header[3..6];
    if version != b"87a" && version != b"89a" {
        return None;
    }

    let mut structs = vec![StructDef {
        name: "GIF Header".into(),
        base_offset: 0,
        fields: vec![
            FieldDef {
                name: "signature".into(),
                offset: 0,
                field_type: FieldType::Bytes(3),
                description: "GIF signature".into(),
                editable: false,
            },
            FieldDef {
                name: "version".into(),
                offset: 3,
                field_type: FieldType::Utf8(3),
                description: "GIF version string".into(),
                editable: true,
            },
        ],
        children: vec![],
    }];

    let logical_screen_offset = GIF_HEADER_LEN;
    let packed = header[10];
    structs.push(StructDef {
        name: "Logical Screen Descriptor".into(),
        base_offset: logical_screen_offset,
        fields: vec![
            FieldDef {
                name: "canvas_width".into(),
                offset: 0,
                field_type: FieldType::U16Le,
                description: "Logical screen width in pixels".into(),
                editable: true,
            },
            FieldDef {
                name: "canvas_height".into(),
                offset: 2,
                field_type: FieldType::U16Le,
                description: "Logical screen height in pixels".into(),
                editable: true,
            },
            FieldDef {
                name: "packed".into(),
                offset: 4,
                field_type: FieldType::Flags {
                    inner: Box::new(FieldType::U8),
                    flags: vec![(0x80, "global_color_table".into()), (0x08, "sorted".into())],
                },
                description: "Logical screen packed flags".into(),
                editable: true,
            },
            FieldDef {
                name: "background_color_index".into(),
                offset: 5,
                field_type: FieldType::U8,
                description: "Background palette index".into(),
                editable: true,
            },
            FieldDef {
                name: "pixel_aspect_ratio".into(),
                offset: 6,
                field_type: FieldType::U8,
                description: "Pixel aspect ratio byte (0 means unspecified)".into(),
                editable: true,
            },
        ],
        children: vec![],
    });

    let mut offset = GIF_HEADER_LEN + LOGICAL_SCREEN_DESCRIPTOR_LEN;
    if packed & 0x80 != 0 {
        let table_len = color_table_len(packed);
        let available = doc.len().saturating_sub(offset);
        let truncated = available < table_len;
        let field_len = available.min(table_len);
        structs.push(StructDef {
            name: if truncated {
                format!("Global Color Table ({} colors, truncated)", table_len / 3)
            } else {
                format!("Global Color Table ({} colors)", table_len / 3)
            },
            base_offset: offset,
            fields: vec![FieldDef {
                name: "global_color_table".into(),
                offset: 0,
                field_type: FieldType::DataRange(field_len),
                description: "RGB palette entries for the logical screen".into(),
                editable: false,
            }],
            children: vec![],
        });
        if truncated {
            return Some(FormatDef {
                name: "GIF".to_string(),
                structs,
            });
        }
        offset += table_len;
    }

    let mut block_count = 0_usize;
    let mut extension_index = 0_usize;
    let mut image_index = 0_usize;
    let mut more_remain = false;

    while offset < doc.len() {
        if block_count >= entry_cap.max(1) {
            if peek_block_start(doc, offset).is_some() {
                more_remain = true;
            }
            break;
        }

        let Some(kind) = peek_block_start(doc, offset) else {
            break;
        };

        match kind {
            TRAILER => {
                structs.push(StructDef {
                    name: "Trailer".into(),
                    base_offset: offset,
                    fields: vec![FieldDef {
                        name: "trailer".into(),
                        offset: 0,
                        field_type: FieldType::Enum {
                            inner: Box::new(FieldType::U8),
                            variants: block_start_variants(),
                        },
                        description: "GIF trailer byte".into(),
                        editable: false,
                    }],
                    children: vec![],
                });
                break;
            }
            IMAGE_SEPARATOR => {
                let parsed = parse_image_block(doc, offset, image_index);
                offset = parsed.next_offset;
                structs.push(parsed.structure);
                block_count += 1;
                image_index += 1;
                if !parsed.complete {
                    break;
                }
            }
            EXTENSION_INTRODUCER => {
                let parsed = parse_extension_block(doc, offset, extension_index);
                offset = parsed.next_offset;
                structs.push(parsed.structure);
                block_count += 1;
                extension_index += 1;
                if !parsed.complete {
                    break;
                }
            }
            other => {
                structs.push(StructDef {
                    name: format!("Unknown block 0x{other:02x} (truncated)"),
                    base_offset: offset,
                    fields: vec![],
                    children: vec![],
                });
                break;
            }
        }
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "… more GIF blocks beyond {} (use `:insp more` to load more)",
                block_count
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "GIF".to_string(),
        structs,
    })
}

struct ParsedBlock {
    structure: StructDef,
    next_offset: u64,
    complete: bool,
}

struct SubBlocksInfo {
    total_len: u64,
    terminated: bool,
}

fn parse_image_block(doc: &mut Document, offset: u64, image_index: usize) -> ParsedBlock {
    let Some(header) = read_bytes_raw(doc, offset, IMAGE_DESCRIPTOR_LEN as usize) else {
        return ParsedBlock {
            structure: StructDef {
                name: format!("Image {image_index} (truncated)"),
                base_offset: offset,
                fields: vec![],
                children: vec![],
            },
            next_offset: doc.len(),
            complete: false,
        };
    };

    let left = u16::from_le_bytes([header[1], header[2]]);
    let top = u16::from_le_bytes([header[3], header[4]]);
    let width = u16::from_le_bytes([header[5], header[6]]);
    let height = u16::from_le_bytes([header[7], header[8]]);
    let packed = header[9];

    let mut fields = vec![
        FieldDef {
            name: "separator".into(),
            offset: 0,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: block_start_variants(),
            },
            description: "Image descriptor separator".into(),
            editable: false,
        },
        FieldDef {
            name: "left".into(),
            offset: 1,
            field_type: FieldType::U16Le,
            description: "Image left position".into(),
            editable: true,
        },
        FieldDef {
            name: "top".into(),
            offset: 3,
            field_type: FieldType::U16Le,
            description: "Image top position".into(),
            editable: true,
        },
        FieldDef {
            name: "width".into(),
            offset: 5,
            field_type: FieldType::U16Le,
            description: "Image width in pixels".into(),
            editable: true,
        },
        FieldDef {
            name: "height".into(),
            offset: 7,
            field_type: FieldType::U16Le,
            description: "Image height in pixels".into(),
            editable: true,
        },
        FieldDef {
            name: "packed".into(),
            offset: 9,
            field_type: FieldType::Flags {
                inner: Box::new(FieldType::U8),
                flags: vec![
                    (0x80, "local_color_table".into()),
                    (0x40, "interlaced".into()),
                    (0x20, "sorted".into()),
                ],
            },
            description: "Image descriptor packed flags".into(),
            editable: true,
        },
    ];

    let mut cursor = offset + IMAGE_DESCRIPTOR_LEN;
    if packed & 0x80 != 0 {
        let table_len = color_table_len(packed);
        let available = doc.len().saturating_sub(cursor);
        let truncated = available < table_len;
        let field_len = available.min(table_len);
        fields.push(FieldDef {
            name: "local_color_table".into(),
            offset: cursor - offset,
            field_type: FieldType::DataRange(field_len),
            description: "RGB palette entries local to this image".into(),
            editable: false,
        });
        cursor += field_len;
        if truncated {
            return ParsedBlock {
                structure: StructDef {
                    name: format!(
                        "Image {image_index}: {width}x{height} @ ({left}, {top}) (truncated)"
                    ),
                    base_offset: offset,
                    fields,
                    children: vec![],
                },
                next_offset: doc.len(),
                complete: false,
            };
        }
    }

    let Some(lzw_min_code_size) = read_u8(doc, cursor) else {
        return ParsedBlock {
            structure: StructDef {
                name: format!(
                    "Image {image_index}: {width}x{height} @ ({left}, {top}) (truncated)"
                ),
                base_offset: offset,
                fields,
                children: vec![],
            },
            next_offset: doc.len(),
            complete: false,
        };
    };
    fields.push(FieldDef {
        name: "lzw_min_code_size".into(),
        offset: cursor - offset,
        field_type: FieldType::U8,
        description: "Minimum LZW code size for image data".into(),
        editable: true,
    });
    let sub_blocks_start = cursor + 1;
    let sub_blocks = scan_sub_blocks(doc, sub_blocks_start);
    fields.push(FieldDef {
        name: "image_data_sub_blocks".into(),
        offset: sub_blocks_start - offset,
        field_type: FieldType::DataRange(sub_blocks.total_len),
        description: format!(
            "Raw image data sub-block stream (min code size 0x{lzw_min_code_size:02x})"
        ),
        editable: false,
    });

    ParsedBlock {
        structure: StructDef {
            name: if sub_blocks.terminated {
                format!("Image {image_index}: {width}x{height} @ ({left}, {top})")
            } else {
                format!("Image {image_index}: {width}x{height} @ ({left}, {top}) (truncated)")
            },
            base_offset: offset,
            fields,
            children: vec![],
        },
        next_offset: sub_blocks_start + sub_blocks.total_len,
        complete: sub_blocks.terminated,
    }
}

fn parse_extension_block(doc: &mut Document, offset: u64, extension_index: usize) -> ParsedBlock {
    let Some(label) = read_u8(doc, offset + 1) else {
        return ParsedBlock {
            structure: StructDef {
                name: format!("Extension {extension_index} (truncated)"),
                base_offset: offset,
                fields: vec![],
                children: vec![],
            },
            next_offset: doc.len(),
            complete: false,
        };
    };

    match label {
        GRAPHIC_CONTROL_LABEL => parse_graphic_control_extension(doc, offset, extension_index),
        APPLICATION_LABEL => parse_application_extension(doc, offset, extension_index),
        PLAINTEXT_LABEL => parse_plaintext_extension(doc, offset, extension_index),
        COMMENT_LABEL => parse_comment_extension(doc, offset, extension_index),
        _ => parse_generic_extension(doc, offset, extension_index, label),
    }
}

fn parse_graphic_control_extension(
    doc: &mut Document,
    offset: u64,
    extension_index: usize,
) -> ParsedBlock {
    let header_len = 2 + 1 + GRAPHIC_CONTROL_DATA_LEN + 1;
    let available = doc.len().saturating_sub(offset);
    let truncated = available < header_len;
    let mut fields = vec![
        FieldDef {
            name: "introducer".into(),
            offset: 0,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: block_start_variants(),
            },
            description: "Extension introducer".into(),
            editable: false,
        },
        FieldDef {
            name: "label".into(),
            offset: 1,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: extension_label_variants(),
            },
            description: "Extension label".into(),
            editable: true,
        },
    ];

    if available >= 3 {
        fields.push(FieldDef {
            name: "block_size".into(),
            offset: 2,
            field_type: FieldType::U8,
            description: "Graphic Control Extension block size (normally 4)".into(),
            editable: true,
        });
    }
    if available >= 4 {
        fields.push(FieldDef {
            name: "packed".into(),
            offset: 3,
            field_type: FieldType::Flags {
                inner: Box::new(FieldType::U8),
                flags: vec![(0x01, "transparent_color".into())],
            },
            description: "Graphic Control Extension packed flags".into(),
            editable: true,
        });
    }
    if available >= 6 {
        fields.push(FieldDef {
            name: "delay_time".into(),
            offset: 4,
            field_type: FieldType::U16Le,
            description: "Frame delay in hundredths of a second".into(),
            editable: true,
        });
    }
    if available >= 7 {
        fields.push(FieldDef {
            name: "transparent_color_index".into(),
            offset: 6,
            field_type: FieldType::U8,
            description: "Transparent palette index when enabled".into(),
            editable: true,
        });
    }
    if available >= 8 {
        fields.push(FieldDef {
            name: "terminator".into(),
            offset: 7,
            field_type: FieldType::U8,
            description: "Graphic Control Extension terminator byte".into(),
            editable: false,
        });
    }

    ParsedBlock {
        structure: StructDef {
            name: if truncated {
                format!("Extension {extension_index}: Graphics Control (truncated)")
            } else {
                format!("Extension {extension_index}: Graphics Control")
            },
            base_offset: offset,
            fields,
            children: vec![],
        },
        next_offset: offset + available.min(header_len),
        complete: !truncated,
    }
}

fn parse_application_extension(
    doc: &mut Document,
    offset: u64,
    extension_index: usize,
) -> ParsedBlock {
    let Some(block_size) = read_u8(doc, offset + 2) else {
        return ParsedBlock {
            structure: StructDef {
                name: format!("Extension {extension_index}: Application (truncated)"),
                base_offset: offset,
                fields: vec![],
                children: vec![],
            },
            next_offset: doc.len(),
            complete: false,
        };
    };

    let header_data_len = block_size as u64;
    let header_total_len = 3 + header_data_len;
    let available = doc.len().saturating_sub(offset);
    let mut fields = vec![
        FieldDef {
            name: "introducer".into(),
            offset: 0,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: block_start_variants(),
            },
            description: "Extension introducer".into(),
            editable: false,
        },
        FieldDef {
            name: "label".into(),
            offset: 1,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: extension_label_variants(),
            },
            description: "Application Extension label".into(),
            editable: true,
        },
        FieldDef {
            name: "block_size".into(),
            offset: 2,
            field_type: FieldType::U8,
            description: "Application Extension fixed header size".into(),
            editable: true,
        },
    ];

    if block_size as usize >= APPLICATION_IDENTIFIER_LEN
        && available >= 3 + APPLICATION_IDENTIFIER_LEN as u64
    {
        fields.push(FieldDef {
            name: "application_identifier".into(),
            offset: 3,
            field_type: FieldType::Utf8(APPLICATION_IDENTIFIER_LEN),
            description: "Application identifier".into(),
            editable: true,
        });
    }
    if block_size as usize >= APPLICATION_IDENTIFIER_LEN + APPLICATION_AUTH_CODE_LEN
        && available >= 3 + (APPLICATION_IDENTIFIER_LEN + APPLICATION_AUTH_CODE_LEN) as u64
    {
        fields.push(FieldDef {
            name: "application_auth_code".into(),
            offset: 3 + APPLICATION_IDENTIFIER_LEN as u64,
            field_type: FieldType::Utf8(APPLICATION_AUTH_CODE_LEN),
            description: "Application authentication code".into(),
            editable: true,
        });
    }

    if available < header_total_len {
        return ParsedBlock {
            structure: StructDef {
                name: format!("Extension {extension_index}: Application (truncated)"),
                base_offset: offset,
                fields,
                children: vec![],
            },
            next_offset: doc.len(),
            complete: false,
        };
    }

    let sub_blocks_start = offset + header_total_len;
    let sub_blocks = scan_sub_blocks(doc, sub_blocks_start);
    fields.push(FieldDef {
        name: "application_data_sub_blocks".into(),
        offset: sub_blocks_start - offset,
        field_type: FieldType::DataRange(sub_blocks.total_len),
        description: "Application-specific sub-block stream".into(),
        editable: false,
    });

    let app_name = read_text(doc, offset + 3, APPLICATION_IDENTIFIER_LEN).unwrap_or_default();
    let app_label = if app_name.is_empty() {
        "Application".to_owned()
    } else {
        format!("Application ({app_name})")
    };

    ParsedBlock {
        structure: StructDef {
            name: if sub_blocks.terminated {
                format!("Extension {extension_index}: {app_label}")
            } else {
                format!("Extension {extension_index}: {app_label} (truncated)")
            },
            base_offset: offset,
            fields,
            children: vec![],
        },
        next_offset: sub_blocks_start + sub_blocks.total_len,
        complete: sub_blocks.terminated,
    }
}

fn parse_plaintext_extension(
    doc: &mut Document,
    offset: u64,
    extension_index: usize,
) -> ParsedBlock {
    let Some(block_size) = read_u8(doc, offset + 2) else {
        return ParsedBlock {
            structure: StructDef {
                name: format!("Extension {extension_index}: Plain Text (truncated)"),
                base_offset: offset,
                fields: vec![],
                children: vec![],
            },
            next_offset: doc.len(),
            complete: false,
        };
    };

    let header_data_len = block_size as u64;
    let header_total_len = 3 + header_data_len;
    let available = doc.len().saturating_sub(offset);
    let mut fields = vec![
        FieldDef {
            name: "introducer".into(),
            offset: 0,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: block_start_variants(),
            },
            description: "Extension introducer".into(),
            editable: false,
        },
        FieldDef {
            name: "label".into(),
            offset: 1,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: extension_label_variants(),
            },
            description: "Plain Text Extension label".into(),
            editable: true,
        },
        FieldDef {
            name: "block_size".into(),
            offset: 2,
            field_type: FieldType::U8,
            description: "Plain Text Extension fixed header size (normally 12)".into(),
            editable: true,
        },
    ];

    if block_size == PLAINTEXT_HEADER_LEN as u8 && available >= header_total_len {
        fields.extend([
            FieldDef {
                name: "text_left".into(),
                offset: 3,
                field_type: FieldType::U16Le,
                description: "Text grid left position".into(),
                editable: true,
            },
            FieldDef {
                name: "text_top".into(),
                offset: 5,
                field_type: FieldType::U16Le,
                description: "Text grid top position".into(),
                editable: true,
            },
            FieldDef {
                name: "text_width".into(),
                offset: 7,
                field_type: FieldType::U16Le,
                description: "Text grid width".into(),
                editable: true,
            },
            FieldDef {
                name: "text_height".into(),
                offset: 9,
                field_type: FieldType::U16Le,
                description: "Text grid height".into(),
                editable: true,
            },
            FieldDef {
                name: "cell_width".into(),
                offset: 11,
                field_type: FieldType::U8,
                description: "Text cell width".into(),
                editable: true,
            },
            FieldDef {
                name: "cell_height".into(),
                offset: 12,
                field_type: FieldType::U8,
                description: "Text cell height".into(),
                editable: true,
            },
            FieldDef {
                name: "foreground_color_index".into(),
                offset: 13,
                field_type: FieldType::U8,
                description: "Foreground palette index".into(),
                editable: true,
            },
            FieldDef {
                name: "background_color_index".into(),
                offset: 14,
                field_type: FieldType::U8,
                description: "Background palette index".into(),
                editable: true,
            },
        ]);
    }

    if available < header_total_len {
        return ParsedBlock {
            structure: StructDef {
                name: format!("Extension {extension_index}: Plain Text (truncated)"),
                base_offset: offset,
                fields,
                children: vec![],
            },
            next_offset: doc.len(),
            complete: false,
        };
    }

    let sub_blocks_start = offset + header_total_len;
    let sub_blocks = scan_sub_blocks(doc, sub_blocks_start);
    fields.push(FieldDef {
        name: "text_sub_blocks".into(),
        offset: sub_blocks_start - offset,
        field_type: FieldType::DataRange(sub_blocks.total_len),
        description: "Plain-text payload sub-block stream".into(),
        editable: false,
    });

    ParsedBlock {
        structure: StructDef {
            name: if sub_blocks.terminated {
                format!("Extension {extension_index}: Plain Text")
            } else {
                format!("Extension {extension_index}: Plain Text (truncated)")
            },
            base_offset: offset,
            fields,
            children: vec![],
        },
        next_offset: sub_blocks_start + sub_blocks.total_len,
        complete: sub_blocks.terminated,
    }
}

fn parse_comment_extension(doc: &mut Document, offset: u64, extension_index: usize) -> ParsedBlock {
    let sub_blocks_start = offset + 2;
    let sub_blocks = scan_sub_blocks(doc, sub_blocks_start);
    ParsedBlock {
        structure: StructDef {
            name: if sub_blocks.terminated {
                format!("Extension {extension_index}: Comment")
            } else {
                format!("Extension {extension_index}: Comment (truncated)")
            },
            base_offset: offset,
            fields: vec![
                FieldDef {
                    name: "introducer".into(),
                    offset: 0,
                    field_type: FieldType::Enum {
                        inner: Box::new(FieldType::U8),
                        variants: block_start_variants(),
                    },
                    description: "Extension introducer".into(),
                    editable: false,
                },
                FieldDef {
                    name: "label".into(),
                    offset: 1,
                    field_type: FieldType::Enum {
                        inner: Box::new(FieldType::U8),
                        variants: extension_label_variants(),
                    },
                    description: "Comment Extension label".into(),
                    editable: true,
                },
                FieldDef {
                    name: "comment_sub_blocks".into(),
                    offset: 2,
                    field_type: FieldType::DataRange(sub_blocks.total_len),
                    description: "Comment text sub-block stream".into(),
                    editable: false,
                },
            ],
            children: vec![],
        },
        next_offset: sub_blocks_start + sub_blocks.total_len,
        complete: sub_blocks.terminated,
    }
}

fn parse_generic_extension(
    doc: &mut Document,
    offset: u64,
    extension_index: usize,
    label: u8,
) -> ParsedBlock {
    let sub_blocks_start = offset + 2;
    let sub_blocks = scan_sub_blocks(doc, sub_blocks_start);
    ParsedBlock {
        structure: StructDef {
            name: if sub_blocks.terminated {
                format!(
                    "Extension {extension_index}: {}",
                    extension_label_name(label)
                )
            } else {
                format!(
                    "Extension {extension_index}: {} (truncated)",
                    extension_label_name(label)
                )
            },
            base_offset: offset,
            fields: vec![
                FieldDef {
                    name: "introducer".into(),
                    offset: 0,
                    field_type: FieldType::Enum {
                        inner: Box::new(FieldType::U8),
                        variants: block_start_variants(),
                    },
                    description: "Extension introducer".into(),
                    editable: false,
                },
                FieldDef {
                    name: "label".into(),
                    offset: 1,
                    field_type: FieldType::Enum {
                        inner: Box::new(FieldType::U8),
                        variants: extension_label_variants(),
                    },
                    description: "Extension label".into(),
                    editable: true,
                },
                FieldDef {
                    name: "extension_sub_blocks".into(),
                    offset: 2,
                    field_type: FieldType::DataRange(sub_blocks.total_len),
                    description: "Raw extension payload sub-block stream".into(),
                    editable: false,
                },
            ],
            children: vec![],
        },
        next_offset: sub_blocks_start + sub_blocks.total_len,
        complete: sub_blocks.terminated,
    }
}

fn peek_block_start(doc: &mut Document, offset: u64) -> Option<u8> {
    read_u8(doc, offset)
}

fn scan_sub_blocks(doc: &mut Document, start: u64) -> SubBlocksInfo {
    if start >= doc.len() {
        return SubBlocksInfo {
            total_len: 0,
            terminated: false,
        };
    }

    let mut cursor = start;
    while cursor < doc.len() {
        let Some(size) = read_u8(doc, cursor) else {
            break;
        };
        cursor += 1;
        if size == 0 {
            return SubBlocksInfo {
                total_len: cursor - start,
                terminated: true,
            };
        }
        let data_len = size as u64;
        if cursor.saturating_add(data_len) > doc.len() {
            return SubBlocksInfo {
                total_len: doc.len().saturating_sub(start),
                terminated: false,
            };
        }
        cursor += data_len;
    }

    SubBlocksInfo {
        total_len: doc.len().saturating_sub(start),
        terminated: false,
    }
}

fn color_table_len(packed: u8) -> u64 {
    let exponent = ((packed & 0x07) + 1) as u32;
    3_u64 * (1_u64 << exponent)
}

fn read_text(doc: &mut Document, offset: u64, len: usize) -> Option<String> {
    let bytes = read_bytes_raw(doc, offset, len)?;
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn block_start_variants() -> Vec<(u64, String)> {
    vec![
        (EXTENSION_INTRODUCER as u64, "Extension".into()),
        (IMAGE_SEPARATOR as u64, "Image Separator".into()),
        (TRAILER as u64, "Trailer".into()),
    ]
}

fn extension_label_variants() -> Vec<(u64, String)> {
    vec![
        (PLAINTEXT_LABEL as u64, "Plain Text".into()),
        (GRAPHIC_CONTROL_LABEL as u64, "Graphics Control".into()),
        (COMMENT_LABEL as u64, "Comment".into()),
        (APPLICATION_LABEL as u64, "Application".into()),
    ]
}

fn extension_label_name(label: u8) -> String {
    extension_label_variants()
        .into_iter()
        .find_map(|(value, name)| (value == label as u64).then_some(name))
        .unwrap_or_else(|| format!("Extension 0x{label:02x}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format;

    fn write_gif(path: &std::path::Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    fn sample_gif_with_frame() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GIF89a");
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.push(0x80);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0xff, 0xff, 0xff]);
        bytes.extend_from_slice(&[0x21, 0xf9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x2c, 0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.push(0x00);
        bytes.push(0x02);
        bytes.extend_from_slice(&[0x02, 0x44, 0x01, 0x00]);
        bytes.push(0x3b);
        bytes
    }

    fn sample_gif_with_many_comments() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GIF89a");
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.push(0x00);
        bytes.push(0);
        bytes.push(0);
        for comment in [b"one".as_slice(), b"two".as_slice(), b"three".as_slice()] {
            bytes.push(0x21);
            bytes.push(0xfe);
            bytes.push(comment.len() as u8);
            bytes.extend_from_slice(comment);
            bytes.push(0x00);
        }
        bytes.push(0x3b);
        bytes
    }

    #[test]
    fn detects_header_palette_and_image_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.gif");
        let mut doc = write_gif(&path, &sample_gif_with_frame());

        let def = detect(&mut doc).expect("gif detected");
        assert_eq!(def.name, "GIF");
        assert_eq!(def.structs[0].name, "GIF Header");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("Global Color Table")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("Graphics Control")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("Image 0: 1x1")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Trailer"));

        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
        let image = structs
            .iter()
            .find(|structure| structure.name.contains("Image 0: 1x1"))
            .expect("image struct");
        let sub_blocks = image
            .fields
            .iter()
            .find(|field| field.def.name == "image_data_sub_blocks")
            .expect("image data field");
        assert_eq!(sub_blocks.size, 4);
    }

    #[test]
    fn paginates_gif_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("many-comments.gif");
        let mut doc = write_gif(&path, &sample_gif_with_many_comments());

        let def = detect_with_cap(&mut doc, 2).expect("gif detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("more GIF blocks beyond 2")));

        let full = detect_with_cap(&mut doc, 8).expect("gif detected");
        assert!(!full
            .structs
            .iter()
            .any(|structure| structure.name.contains("more GIF blocks")));
        assert!(full
            .structs
            .iter()
            .any(|structure| structure.name == "Trailer"));
    }

    #[test]
    fn keeps_detecting_truncated_gif_after_magic_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("truncated.gif");
        let mut bytes = sample_gif_with_frame();
        bytes.truncate(20);
        let mut doc = write_gif(&path, &bytes);

        let def = detect(&mut doc).expect("gif still detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("truncated")));
    }
}
