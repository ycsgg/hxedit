use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const GZIP_CM_DEFLATE: u8 = 8;

const FTEXT: u8 = 0x01;
const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, _entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < 10 {
        return None;
    }

    let fixed = read_bytes_raw(doc, 0, 10)?;
    if fixed[0..2] != GZIP_MAGIC || fixed[2] != GZIP_CM_DEFLATE {
        return None;
    }

    let flags = fixed[3];
    let mut fields = vec![
        FieldDef {
            name: "signature".into(),
            offset: 0,
            field_type: FieldType::Bytes(2),
            description: "Gzip file signature".into(),
            editable: false,
        },
        FieldDef {
            name: "compression_method".into(),
            offset: 2,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: vec![(GZIP_CM_DEFLATE as u64, "Deflate".into())],
            },
            description: "Compression method".into(),
            editable: true,
        },
        FieldDef {
            name: "flags".into(),
            offset: 3,
            field_type: FieldType::Flags {
                inner: Box::new(FieldType::U8),
                flags: vec![
                    (FTEXT as u64, "FTEXT".into()),
                    (FHCRC as u64, "FHCRC".into()),
                    (FEXTRA as u64, "FEXTRA".into()),
                    (FNAME as u64, "FNAME".into()),
                    (FCOMMENT as u64, "FCOMMENT".into()),
                ],
            },
            description: "Gzip header flags".into(),
            editable: true,
        },
        FieldDef {
            name: "mtime".into(),
            offset: 4,
            field_type: FieldType::U32Le,
            description: "Modification time".into(),
            editable: true,
        },
        FieldDef {
            name: "xfl".into(),
            offset: 8,
            field_type: FieldType::U8,
            description: "Extra flags".into(),
            editable: true,
        },
        FieldDef {
            name: "os".into(),
            offset: 9,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: os_variants(),
            },
            description: "Originating operating system".into(),
            editable: true,
        },
    ];

    let mut cursor = 10_u64;
    let mut truncated = false;

    if flags & FEXTRA != 0 {
        let Some(xlen) = read_u16_le(doc, cursor) else {
            return Some(single_header("GZIP", "Gzip Header (truncated)", fields));
        };
        fields.push(FieldDef {
            name: "extra_len".into(),
            offset: cursor,
            field_type: FieldType::U16Le,
            description: "Extra field length".into(),
            editable: true,
        });
        cursor += 2;
        if cursor.saturating_add(xlen as u64) > doc.len() {
            truncated = true;
        } else if xlen > 0 {
            fields.push(FieldDef {
                name: "extra_data".into(),
                offset: cursor,
                field_type: FieldType::DataRange(xlen as u64),
                description: "Extra field payload".into(),
                editable: false,
            });
            cursor += xlen as u64;
        }
    }

    if !truncated && flags & FNAME != 0 {
        if let Some(field) = c_string_field(doc, cursor, "filename", "Original filename") {
            cursor += field.field_type.byte_size().unwrap_or(0) as u64;
            fields.push(field);
        } else {
            truncated = true;
        }
    }

    if !truncated && flags & FCOMMENT != 0 {
        if let Some(field) = c_string_field(doc, cursor, "comment", "Header comment") {
            cursor += field.field_type.byte_size().unwrap_or(0) as u64;
            fields.push(field);
        } else {
            truncated = true;
        }
    }

    if !truncated && flags & FHCRC != 0 {
        if cursor.saturating_add(2) > doc.len() {
            truncated = true;
        } else {
            fields.push(FieldDef {
                name: "header_crc16".into(),
                offset: cursor,
                field_type: FieldType::U16Le,
                description: "CRC16 over the gzip header".into(),
                editable: true,
            });
            cursor += 2;
        }
    }

    let header_name = if truncated {
        "Gzip Header (truncated)"
    } else {
        "Gzip Header"
    };
    let mut structs = vec![StructDef {
        name: header_name.into(),
        base_offset: 0,
        fields,
        children: vec![],
    }];

    if truncated {
        return Some(FormatDef {
            name: "GZIP".to_string(),
            structs,
        });
    }

    let compressed_and_trailer = doc.len().saturating_sub(cursor);
    if compressed_and_trailer < 8 {
        structs.push(StructDef {
            name: "Gzip Trailer (truncated)".into(),
            base_offset: cursor,
            fields: vec![],
            children: vec![],
        });
        return Some(FormatDef {
            name: "GZIP".to_string(),
            structs,
        });
    }

    let compressed_len = compressed_and_trailer - 8;
    if compressed_len > 0 {
        structs.push(StructDef {
            name: "Compressed Data".into(),
            base_offset: cursor,
            fields: vec![FieldDef {
                name: "compressed_data".into(),
                offset: 0,
                field_type: FieldType::DataRange(compressed_len),
                description: "Compressed DEFLATE payload".into(),
                editable: false,
            }],
            children: vec![],
        });
    }

    let trailer_offset = doc.len() - 8;
    structs.push(StructDef {
        name: "Gzip Trailer".into(),
        base_offset: trailer_offset,
        fields: vec![
            FieldDef {
                name: "crc32".into(),
                offset: 0,
                field_type: FieldType::U32Le,
                description: "CRC32 of the uncompressed data".into(),
                editable: true,
            },
            FieldDef {
                name: "isize".into(),
                offset: 4,
                field_type: FieldType::U32Le,
                description: "Uncompressed size modulo 2^32".into(),
                editable: true,
            },
        ],
        children: vec![],
    });

    Some(FormatDef {
        name: "GZIP".to_string(),
        structs,
    })
}

fn single_header(format_name: &str, name: &str, fields: Vec<FieldDef>) -> FormatDef {
    FormatDef {
        name: format_name.to_string(),
        structs: vec![StructDef {
            name: name.to_string(),
            base_offset: 0,
            fields,
            children: vec![],
        }],
    }
}

fn read_u16_le(doc: &mut Document, offset: u64) -> Option<u16> {
    let bytes = read_bytes_raw(doc, offset, 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn c_string_field(
    doc: &mut Document,
    offset: u64,
    name: &str,
    description: &str,
) -> Option<FieldDef> {
    let len = c_string_len(doc, offset)?;
    Some(FieldDef {
        name: name.to_string(),
        offset,
        field_type: FieldType::Utf8(len),
        description: description.to_string(),
        editable: true,
    })
}

fn c_string_len(doc: &mut Document, offset: u64) -> Option<usize> {
    if offset >= doc.len() {
        return None;
    }

    let mut cursor = offset;
    let mut total = 0_usize;
    while cursor < doc.len() {
        let batch = read_bytes_raw(doc, cursor, (doc.len() - cursor).min(256) as usize)?;
        if let Some(nul) = batch.iter().position(|&byte| byte == 0) {
            return Some(total + nul + 1);
        }
        total += batch.len();
        cursor += batch.len() as u64;
    }
    None
}

fn os_variants() -> Vec<(u64, String)> {
    vec![
        (0, "FAT".into()),
        (1, "Amiga".into()),
        (2, "VMS".into()),
        (3, "Unix".into()),
        (4, "VM/CMS".into()),
        (5, "Atari TOS".into()),
        (6, "HPFS".into()),
        (7, "Macintosh".into()),
        (8, "Z-System".into()),
        (9, "CP/M".into()),
        (10, "TOPS-20".into()),
        (11, "NTFS".into()),
        (12, "QDOS".into()),
        (13, "Acorn RISCOS".into()),
        (255, "Unknown".into()),
    ]
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap, GZIP_CM_DEFLATE, GZIP_MAGIC};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format;

    fn write_gzip(path: &std::path::Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    fn build_gzip_with_optional_fields() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&GZIP_MAGIC);
        bytes.push(GZIP_CM_DEFLATE);
        bytes.push(super::FHCRC | super::FEXTRA | super::FNAME | super::FCOMMENT);
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes.push(2);
        bytes.push(3);
        bytes.extend_from_slice(&3_u16.to_le_bytes());
        bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        bytes.extend_from_slice(b"hello.txt\0");
        bytes.extend_from_slice(b"sample comment\0");
        bytes.extend_from_slice(&0x9a7b_u16.to_le_bytes());
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        bytes.extend_from_slice(&0xdead_beef_u32.to_le_bytes());
        bytes.extend_from_slice(&0x42_u32.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_optional_header_fields_and_trailer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.gz");
        let mut doc = write_gzip(&path, &build_gzip_with_optional_fields());

        let def = detect(&mut doc).expect("gzip detected");
        assert_eq!(def.name, "GZIP");
        assert_eq!(def.structs[0].name, "Gzip Header");
        assert!(def.structs[0]
            .fields
            .iter()
            .any(|field| field.name == "extra_data"));
        assert!(def.structs[0]
            .fields
            .iter()
            .any(|field| field.name == "filename"));
        assert!(def.structs[0]
            .fields
            .iter()
            .any(|field| field.name == "comment"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Compressed Data"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Gzip Trailer"));

        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
        let compressed = structs
            .iter()
            .find(|structure| structure.name == "Compressed Data")
            .expect("compressed data struct");
        let field = compressed
            .fields
            .iter()
            .find(|field| field.def.name == "compressed_data")
            .expect("compressed data range");
        assert_eq!(field.size, 4);
    }

    #[test]
    fn rejects_non_deflate_gzip_members() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.gz");
        let mut bytes = vec![0; 18];
        bytes[0..2].copy_from_slice(&GZIP_MAGIC);
        bytes[2] = 0;
        let mut doc = write_gzip(&path, &bytes);
        assert!(detect(&mut doc).is_none());
    }

    #[test]
    fn marks_header_truncated_when_c_string_lacks_nul() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("truncated.gz");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&GZIP_MAGIC);
        bytes.push(GZIP_CM_DEFLATE);
        bytes.push(super::FNAME);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.push(0);
        bytes.push(255);
        bytes.extend_from_slice(b"unterminated");
        let mut doc = write_gzip(&path, &bytes);

        let def = detect_with_cap(&mut doc, 64).expect("gzip detected");
        assert_eq!(def.structs.len(), 1);
        assert!(def.structs[0].name.contains("truncated"));
    }
}
