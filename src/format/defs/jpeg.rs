use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

const JPEG_SOI: [u8; 2] = [0xff, 0xd8];
const SCAN_CHUNK: usize = 8 * 1024;

fn field(
    name: &str,
    offset: u64,
    field_type: FieldType,
    description: &str,
    editable: bool,
) -> FieldDef {
    FieldDef {
        name: name.into(),
        offset,
        field_type,
        description: description.into(),
        editable,
    }
}

fn bytes_field(name: &str, offset: u64, len: usize, description: &str, editable: bool) -> FieldDef {
    field(name, offset, FieldType::Bytes(len), description, editable)
}

fn utf8_field(name: &str, offset: u64, len: usize, description: &str, editable: bool) -> FieldDef {
    field(name, offset, FieldType::Utf8(len), description, editable)
}

fn u8_field(name: &str, offset: u64, description: &str, editable: bool) -> FieldDef {
    field(name, offset, FieldType::U8, description, editable)
}

fn u16be_field(name: &str, offset: u64, description: &str, editable: bool) -> FieldDef {
    field(name, offset, FieldType::U16Be, description, editable)
}

fn enum_u8_field(
    name: &str,
    offset: u64,
    variants: Vec<(u64, String)>,
    description: &str,
    editable: bool,
) -> FieldDef {
    field(
        name,
        offset,
        FieldType::Enum {
            inner: Box::new(FieldType::U8),
            variants,
        },
        description,
        editable,
    )
}

fn data_range_field(name: &str, offset: u64, len: u64, description: &str) -> FieldDef {
    field(name, offset, FieldType::DataRange(len), description, false)
}

fn marker_fields(include_length: bool) -> Vec<FieldDef> {
    let mut fields = vec![
        bytes_field("marker_prefix", 0, 1, "JPEG marker prefix", false),
        enum_u8_field("marker", 1, marker_variants(), "JPEG marker code", true),
    ];
    if include_length {
        fields.push(u16be_field(
            "length",
            2,
            "Segment length including the length field",
            true,
        ));
    }
    fields
}

fn struct_with_fields(
    name: impl Into<String>,
    base_offset: u64,
    fields: Vec<FieldDef>,
) -> StructDef {
    StructDef {
        name: name.into(),
        base_offset,
        fields,
        children: vec![],
    }
}

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < 4 {
        return None;
    }

    let soi = read_bytes_raw(doc, 0, 2)?;
    if soi != JPEG_SOI {
        return None;
    }

    let mut structs = vec![standalone_marker_struct("Marker 0: SOI", 0)];
    let mut offset = 2_u64;
    let mut marker_idx = 1_usize;
    let mut scan_idx = 0_usize;
    let mut more_remain = false;

    while offset.saturating_add(2) <= doc.len() {
        if marker_idx >= entry_cap.max(1) {
            if let Some(marker) = peek_marker(doc, offset) {
                if marker != 0xd9 {
                    more_remain = true;
                }
            }
            break;
        }

        let Some(marker) = peek_marker(doc, offset) else {
            break;
        };
        if marker == 0xff || marker == 0x00 {
            break;
        }

        if marker_is_standalone(marker) {
            structs.push(standalone_marker_struct(
                &format!("Marker {}: {}", marker_idx, marker_label(marker)),
                offset,
            ));
            offset += 2;
            marker_idx += 1;
            if marker == 0xd9 {
                break;
            }
            continue;
        }

        let Some(length_bytes) = read_bytes_raw(doc, offset + 2, 2) else {
            break;
        };
        let length = u16::from_be_bytes([length_bytes[0], length_bytes[1]]) as u64;
        if length < 2 {
            break;
        }

        let data_len = length - 2;
        let segment_end = offset.saturating_add(2 + length);
        let truncated = segment_end > doc.len();
        let mut fields = marker_fields(true);

        extend_segment_fields(doc, offset, marker, data_len, &mut fields);

        if !truncated && data_len > 0 {
            fields.push(data_range_field(
                "segment_data",
                4,
                data_len,
                "Raw segment payload bytes",
            ));
        }

        let segment_name = segment_display_name(doc, offset, marker);
        let name = if truncated {
            format!("Marker {}: {} (truncated)", marker_idx, segment_name)
        } else {
            format!("Marker {}: {}", marker_idx, segment_name)
        };
        structs.push(struct_with_fields(name, offset, fields));
        marker_idx += 1;

        if truncated {
            break;
        }

        if marker == 0xda {
            let scan_start = segment_end;
            let scan_end = find_scan_end(doc, scan_start);
            let scan_len = scan_end.saturating_sub(scan_start);
            if scan_len > 0 {
                structs.push(struct_with_fields(
                    format!("Scan Data {}", scan_idx),
                    scan_start,
                    vec![data_range_field(
                        "scan_data",
                        0,
                        scan_len,
                        "Entropy-coded scan data",
                    )],
                ));
                scan_idx += 1;
            }
            offset = scan_end;
        } else {
            offset = segment_end;
        }
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "… more markers beyond {} (use `:insp more` to load more)",
                marker_idx
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "JPEG".to_string(),
        structs,
    })
}

fn peek_marker(doc: &mut Document, offset: u64) -> Option<u8> {
    let bytes = read_bytes_raw(doc, offset, 2)?;
    if bytes[0] != 0xff {
        return None;
    }
    Some(bytes[1])
}

fn standalone_marker_struct(name: &str, offset: u64) -> StructDef {
    struct_with_fields(name, offset, marker_fields(false))
}

fn segment_display_name(doc: &mut Document, offset: u64, marker: u8) -> String {
    match marker {
        0xe0..=0xef => {
            let identifier = read_identifier(doc, offset + 4, 8);
            if identifier.is_empty() {
                marker_label(marker).to_string()
            } else {
                format!("{} ({identifier})", marker_label(marker))
            }
        }
        0xfe => {
            let comment = read_identifier(doc, offset + 4, 24);
            if comment.is_empty() {
                "COM".to_string()
            } else {
                format!("COM ({comment})")
            }
        }
        _ => marker_label(marker).to_string(),
    }
}

fn extend_segment_fields(
    doc: &mut Document,
    offset: u64,
    marker: u8,
    data_len: u64,
    fields: &mut Vec<FieldDef>,
) {
    match marker {
        0xe0 => {
            if data_len >= 5 {
                fields.push(utf8_field("identifier", 4, 5, "APP0 identifier", true));
            }
            if data_len >= 14 && read_identifier(doc, offset + 4, 5) == "JFIF" {
                fields.extend([
                    u8_field("version_major", 9, "JFIF major version", true),
                    u8_field("version_minor", 10, "JFIF minor version", true),
                    enum_u8_field(
                        "density_units",
                        11,
                        vec![
                            (0, "None".into()),
                            (1, "Dots per inch".into()),
                            (2, "Dots per cm".into()),
                        ],
                        "JFIF density units",
                        true,
                    ),
                    u16be_field("x_density", 12, "JFIF horizontal density", true),
                    u16be_field("y_density", 14, "JFIF vertical density", true),
                ]);
            }
        }
        0xe1 => {
            if data_len >= 6 {
                fields.push(utf8_field("identifier", 4, 6, "APP1 identifier", true));
            }
        }
        0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
            if data_len >= 6 {
                fields.extend([
                    u8_field("precision", 4, "Sample precision", true),
                    u16be_field("height", 5, "Image height", true),
                    u16be_field("width", 7, "Image width", true),
                    u8_field("components", 9, "Number of image components", true),
                ]);
            }
        }
        0xda => {
            if data_len >= 3 {
                fields.push(u8_field("components", 4, "Number of scan components", true));
                if let Some(count) =
                    read_bytes_raw(doc, offset + 4, 1).and_then(|bytes| bytes.first().copied())
                {
                    let selectors = count as u64 * 2;
                    if data_len >= selectors + 4 {
                        fields.extend([
                            u8_field(
                                "spectral_start",
                                5 + selectors,
                                "Spectral selection start",
                                true,
                            ),
                            u8_field(
                                "spectral_end",
                                6 + selectors,
                                "Spectral selection end",
                                true,
                            ),
                            u8_field(
                                "approximation",
                                7 + selectors,
                                "Successive approximation bits",
                                true,
                            ),
                        ]);
                    }
                }
            }
        }
        0xdd => {
            if data_len >= 2 {
                fields.push(u16be_field(
                    "restart_interval",
                    4,
                    "Restart interval in MCU blocks",
                    true,
                ));
            }
        }
        0xfe => {
            if data_len > 0 {
                fields.push(utf8_field(
                    "comment",
                    4,
                    data_len as usize,
                    "Comment text",
                    true,
                ));
            }
        }
        _ => {}
    }
}

fn read_identifier(doc: &mut Document, offset: u64, max_len: usize) -> String {
    let Some(bytes) = read_bytes_raw(doc, offset, max_len) else {
        return String::new();
    };
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn marker_is_standalone(marker: u8) -> bool {
    matches!(marker, 0x01 | 0xd0..=0xd9)
}

fn marker_variants() -> Vec<(u64, String)> {
    let mut variants = vec![
        (0xd8, "SOI".into()),
        (0xd9, "EOI".into()),
        (0xda, "SOS".into()),
        (0xdb, "DQT".into()),
        (0xc4, "DHT".into()),
        (0xdd, "DRI".into()),
        (0xfe, "COM".into()),
    ];
    for marker in [
        0xc0_u8, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf,
    ] {
        variants.push((marker as u64, marker_label(marker).into()));
    }
    for marker in 0xe0_u8..=0xef {
        variants.push((marker as u64, marker_label(marker).into()));
    }
    for marker in 0xd0_u8..=0xd7 {
        variants.push((marker as u64, marker_label(marker).into()));
    }
    variants
}

fn marker_label(marker: u8) -> &'static str {
    match marker {
        0x01 => "TEM",
        0xc0 => "SOF0",
        0xc1 => "SOF1",
        0xc2 => "SOF2",
        0xc3 => "SOF3",
        0xc4 => "DHT",
        0xc5 => "SOF5",
        0xc6 => "SOF6",
        0xc7 => "SOF7",
        0xc9 => "SOF9",
        0xca => "SOF10",
        0xcb => "SOF11",
        0xcd => "SOF13",
        0xce => "SOF14",
        0xcf => "SOF15",
        0xd0 => "RST0",
        0xd1 => "RST1",
        0xd2 => "RST2",
        0xd3 => "RST3",
        0xd4 => "RST4",
        0xd5 => "RST5",
        0xd6 => "RST6",
        0xd7 => "RST7",
        0xd8 => "SOI",
        0xd9 => "EOI",
        0xda => "SOS",
        0xdb => "DQT",
        0xdd => "DRI",
        0xe0 => "APP0",
        0xe1 => "APP1",
        0xe2 => "APP2",
        0xe3 => "APP3",
        0xe4 => "APP4",
        0xe5 => "APP5",
        0xe6 => "APP6",
        0xe7 => "APP7",
        0xe8 => "APP8",
        0xe9 => "APP9",
        0xea => "APP10",
        0xeb => "APP11",
        0xec => "APP12",
        0xed => "APP13",
        0xee => "APP14",
        0xef => "APP15",
        0xfe => "COM",
        _ => "MARKER",
    }
}

fn find_scan_end(doc: &mut Document, start: u64) -> u64 {
    let mut cursor = start;
    let mut prev_ff = false;

    while cursor < doc.len() {
        let batch_len = (doc.len() - cursor).min(SCAN_CHUNK as u64) as usize;
        let Some(bytes) = read_bytes_raw(doc, cursor, batch_len) else {
            break;
        };
        if bytes.is_empty() {
            break;
        }

        for (idx, &byte) in bytes.iter().enumerate() {
            if prev_ff {
                match byte {
                    0x00 => prev_ff = false,
                    0xff => prev_ff = true,
                    0xd0..=0xd7 => prev_ff = false,
                    _ => return cursor + idx as u64 - 1,
                }
            } else {
                prev_ff = byte == 0xff;
            }
        }

        cursor += bytes.len() as u64;
    }

    doc.len()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format;

    fn write_jpeg(path: &std::path::Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    fn push_segment(bytes: &mut Vec<u8>, marker: u8, payload: &[u8]) {
        bytes.extend_from_slice(&[0xff, marker]);
        bytes.extend_from_slice(&(payload.len() as u16 + 2).to_be_bytes());
        bytes.extend_from_slice(payload);
    }

    fn build_jpeg() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0xff, 0xd8]);
        push_segment(
            &mut bytes,
            0xe0,
            b"JFIF\0\x01\x02\x00\x00\x48\x00\x48\x00\x00",
        );
        push_segment(
            &mut bytes,
            0xc0,
            &[
                8, 0x00, 0x10, 0x00, 0x20, 3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1,
            ],
        );
        push_segment(&mut bytes, 0xda, &[3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0]);
        bytes.extend_from_slice(&[0x11, 0x22, 0xff, 0x00, 0x33, 0xff, 0xd0, 0x44]);
        bytes.extend_from_slice(&[0xff, 0xd9]);
        bytes
    }

    #[test]
    fn detects_segments_and_scan_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.jpg");
        let mut doc = write_jpeg(&path, &build_jpeg());

        let def = detect(&mut doc).expect("jpeg detected");
        assert_eq!(def.name, "JPEG");
        assert!(def.structs[0].name.contains("SOI"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("APP0")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("SOF0")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("SOS")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("Scan Data")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("EOI")));
    }

    #[test]
    fn scan_data_range_points_to_entropy_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan.jpg");
        let mut doc = write_jpeg(&path, &build_jpeg());

        let def = detect(&mut doc).expect("jpeg detected");
        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
        let scan = structs
            .iter()
            .find(|structure| structure.name == "Scan Data 0")
            .expect("scan data struct");
        let field = scan
            .fields
            .iter()
            .find(|field| field.def.name == "scan_data")
            .expect("scan_data field");
        assert_eq!(field.size, 8);
    }

    #[test]
    fn emits_more_marker_when_segment_cap_is_reached() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("many.jpg");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0xff, 0xd8]);
        for _ in 0..5 {
            push_segment(&mut bytes, 0xfe, b"note");
        }
        bytes.extend_from_slice(&[0xff, 0xd9]);
        let mut doc = write_jpeg(&path, &bytes);

        let def = detect_with_cap(&mut doc, 2).expect("jpeg detected");
        assert!(def
            .structs
            .last()
            .is_some_and(|last| last.name.contains("more markers")));
    }
}
