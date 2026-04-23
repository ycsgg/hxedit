use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

const BITMAP_FILE_HEADER_LEN: u64 = 14;
const CORE_HEADER_LEN: u64 = 12;
const INFO_HEADER_LEN: u64 = 40;
const V2_INFO_HEADER_LEN: u64 = 52;
const V3_INFO_HEADER_LEN: u64 = 56;
const V4_INFO_HEADER_LEN: u64 = 108;
const V5_INFO_HEADER_LEN: u64 = 124;

const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;
const BI_JPEG: u32 = 4;
const BI_PNG: u32 = 5;
const BI_ALPHABITFIELDS: u32 = 6;
const BI_CMYK: u32 = 11;
const BI_CMYKRLE8: u32 = 12;
const BI_CMYKRLE4: u32 = 13;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, _entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < BITMAP_FILE_HEADER_LEN + 4 {
        return None;
    }

    let file_header = read_bytes_raw(doc, 0, BITMAP_FILE_HEADER_LEN as usize)?;
    if &file_header[0..2] != b"BM" {
        return None;
    }

    let declared_file_size = u32::from_le_bytes([
        file_header[2],
        file_header[3],
        file_header[4],
        file_header[5],
    ]) as u64;
    let pixel_offset = u32::from_le_bytes([
        file_header[10],
        file_header[11],
        file_header[12],
        file_header[13],
    ]) as u64;

    let dib_size = read_u32_le(doc, BITMAP_FILE_HEADER_LEN)? as u64;
    if dib_size < CORE_HEADER_LEN {
        return None;
    }

    let mut structs = vec![StructDef {
        name: "Bitmap File Header".into(),
        base_offset: 0,
        fields: vec![
            FieldDef {
                name: "signature".into(),
                offset: 0,
                field_type: FieldType::Bytes(2),
                description: "BMP file signature".into(),
                editable: false,
            },
            FieldDef {
                name: "file_size".into(),
                offset: 2,
                field_type: FieldType::U32Le,
                description: "Declared file size in bytes".into(),
                editable: true,
            },
            FieldDef {
                name: "reserved1".into(),
                offset: 6,
                field_type: FieldType::U16Le,
                description: "Reserved field".into(),
                editable: true,
            },
            FieldDef {
                name: "reserved2".into(),
                offset: 8,
                field_type: FieldType::U16Le,
                description: "Reserved field".into(),
                editable: true,
            },
            FieldDef {
                name: "pixel_data_offset".into(),
                offset: 10,
                field_type: FieldType::U32Le,
                description: "Absolute file offset of the pixel array".into(),
                editable: true,
            },
        ],
        children: vec![],
    }];

    let dib_offset = BITMAP_FILE_HEADER_LEN;
    let dib_available = doc.len().saturating_sub(dib_offset);
    let dib_present_len = dib_available.min(dib_size);
    let dib_struct = build_dib_header_struct(dib_size, dib_present_len);
    let dib_complete = dib_present_len >= dib_size;
    structs.push(dib_struct);

    if !dib_complete {
        return Some(FormatDef {
            name: "BMP".to_string(),
            structs,
        });
    }

    let compression = if dib_size >= INFO_HEADER_LEN {
        read_u32_le(doc, dib_offset + 16).unwrap_or(BI_RGB)
    } else {
        BI_RGB
    };
    let bits_per_pixel = if dib_size == CORE_HEADER_LEN {
        read_u16_le(doc, dib_offset + 10).unwrap_or(0)
    } else if dib_size >= INFO_HEADER_LEN {
        read_u16_le(doc, dib_offset + 14).unwrap_or(0)
    } else {
        0
    };

    let external_masks_offset = dib_offset + dib_size;
    let external_mask_count = if dib_size == INFO_HEADER_LEN {
        match compression {
            BI_BITFIELDS => 3,
            BI_ALPHABITFIELDS => 4,
            _ => 0,
        }
    } else {
        0
    };
    let external_masks_len = external_mask_count as u64 * 4;
    let external_masks_available = doc.len().saturating_sub(external_masks_offset);
    if external_mask_count > 0 {
        let field_count = (external_masks_available.min(external_masks_len) / 4) as usize;
        let mut mask_fields = Vec::new();
        for (index, name) in ["red_mask", "green_mask", "blue_mask", "alpha_mask"]
            .into_iter()
            .take(field_count)
            .enumerate()
        {
            mask_fields.push(FieldDef {
                name: name.into(),
                offset: (index * 4) as u64,
                field_type: FieldType::U32Le,
                description: "Bit mask for this channel".into(),
                editable: true,
            });
        }
        if external_masks_available < external_masks_len {
            let trailing = external_masks_available.saturating_sub(field_count as u64 * 4);
            if trailing > 0 {
                mask_fields.push(FieldDef {
                    name: "mask_bytes".into(),
                    offset: (field_count * 4) as u64,
                    field_type: FieldType::DataRange(trailing),
                    description: "Truncated raw mask bytes".into(),
                    editable: false,
                });
            }
        }
        structs.push(StructDef {
            name: if external_masks_available >= external_masks_len {
                "Bit Masks".into()
            } else {
                "Bit Masks (truncated)".into()
            },
            base_offset: external_masks_offset,
            fields: mask_fields,
            children: vec![],
        });
        if external_masks_available < external_masks_len {
            return Some(FormatDef {
                name: "BMP".to_string(),
                structs,
            });
        }
    }

    let palette_offset = external_masks_offset + external_masks_len;
    let logical_file_end = bounded_file_end(doc.len(), declared_file_size, palette_offset);
    if pixel_offset > palette_offset {
        let palette_end = logical_file_end.min(pixel_offset);
        let palette_len = palette_end.saturating_sub(palette_offset);
        if palette_len > 0 {
            let entry_size = if dib_size == CORE_HEADER_LEN { 3 } else { 4 };
            let entry_count = palette_len / entry_size;
            structs.push(StructDef {
                name: format!("Color Palette ({} entries)", entry_count),
                base_offset: palette_offset,
                fields: vec![FieldDef {
                    name: "palette_bytes".into(),
                    offset: 0,
                    field_type: FieldType::DataRange(palette_len),
                    description: if bits_per_pixel == 0 {
                        "Raw palette bytes before the pixel array".into()
                    } else {
                        format!(
                            "Palette bytes before the pixel array for {} bpp data",
                            bits_per_pixel
                        )
                    },
                    editable: false,
                }],
                children: vec![],
            });
        }
    }

    if pixel_offset < logical_file_end {
        structs.push(StructDef {
            name: if declared_file_size > doc.len() {
                "Pixel Data (truncated)".into()
            } else {
                "Pixel Data".into()
            },
            base_offset: pixel_offset,
            fields: vec![FieldDef {
                name: "pixel_data".into(),
                offset: 0,
                field_type: FieldType::DataRange(logical_file_end - pixel_offset),
                description: "Raw bitmap pixel bytes".into(),
                editable: false,
            }],
            children: vec![],
        });
    } else if pixel_offset > doc.len() {
        structs.push(StructDef {
            name: "Pixel Data (offset beyond EOF)".into(),
            base_offset: pixel_offset,
            fields: vec![],
            children: vec![],
        });
    }

    if logical_file_end < doc.len() {
        structs.push(StructDef {
            name: "Trailing Data".into(),
            base_offset: logical_file_end,
            fields: vec![FieldDef {
                name: "trailing_bytes".into(),
                offset: 0,
                field_type: FieldType::DataRange(doc.len() - logical_file_end),
                description: "Bytes beyond the BMP file size declared in the header".into(),
                editable: false,
            }],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "BMP".to_string(),
        structs,
    })
}

fn build_dib_header_struct(dib_size: u64, present_len: u64) -> StructDef {
    let mut fields = vec![FieldDef {
        name: "header_size".into(),
        offset: 0,
        field_type: FieldType::U32Le,
        description: "DIB header size in bytes".into(),
        editable: true,
    }];

    if dib_size == CORE_HEADER_LEN {
        push_if_fits(
            &mut fields,
            present_len,
            "width",
            4,
            FieldType::U16Le,
            "Bitmap width in pixels",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "height",
            6,
            FieldType::U16Le,
            "Bitmap height in pixels",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "planes",
            8,
            FieldType::U16Le,
            "Color planes (must be 1)",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "bits_per_pixel",
            10,
            FieldType::U16Le,
            "Bits per pixel",
            true,
        );
    } else if dib_size >= INFO_HEADER_LEN {
        push_if_fits(
            &mut fields,
            present_len,
            "width",
            4,
            FieldType::I32Le,
            "Bitmap width in pixels",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "height",
            8,
            FieldType::I32Le,
            "Bitmap height in pixels; negative means top-down",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "planes",
            12,
            FieldType::U16Le,
            "Color planes (must be 1)",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "bits_per_pixel",
            14,
            FieldType::U16Le,
            "Bits per pixel",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "compression",
            16,
            FieldType::Enum {
                inner: Box::new(FieldType::U32Le),
                variants: compression_variants(),
            },
            "Compression method",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "image_size",
            20,
            FieldType::U32Le,
            "Declared bitmap data size in bytes",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "x_pixels_per_meter",
            24,
            FieldType::I32Le,
            "Horizontal print resolution",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "y_pixels_per_meter",
            28,
            FieldType::I32Le,
            "Vertical print resolution",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "colors_used",
            32,
            FieldType::U32Le,
            "Palette entries actually used",
            true,
        );
        push_if_fits(
            &mut fields,
            present_len,
            "important_colors",
            36,
            FieldType::U32Le,
            "Important palette entries",
            true,
        );

        if dib_size >= V2_INFO_HEADER_LEN {
            push_if_fits(
                &mut fields,
                present_len,
                "red_mask",
                40,
                FieldType::U32Le,
                "Red channel bit mask",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "green_mask",
                44,
                FieldType::U32Le,
                "Green channel bit mask",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "blue_mask",
                48,
                FieldType::U32Le,
                "Blue channel bit mask",
                true,
            );
        }
        if dib_size >= V3_INFO_HEADER_LEN {
            push_if_fits(
                &mut fields,
                present_len,
                "alpha_mask",
                52,
                FieldType::U32Le,
                "Alpha channel bit mask",
                true,
            );
        }
        if dib_size >= V4_INFO_HEADER_LEN {
            push_if_fits(
                &mut fields,
                present_len,
                "color_space_type",
                56,
                FieldType::U32Le,
                "Color space type identifier",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "cie_endpoints",
                60,
                FieldType::DataRange(36),
                "Raw CIEXYZ endpoint triples",
                false,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "gamma_red",
                96,
                FieldType::U32Le,
                "Red gamma value",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "gamma_green",
                100,
                FieldType::U32Le,
                "Green gamma value",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "gamma_blue",
                104,
                FieldType::U32Le,
                "Blue gamma value",
                true,
            );
        }
        if dib_size >= V5_INFO_HEADER_LEN {
            push_if_fits(
                &mut fields,
                present_len,
                "intent",
                108,
                FieldType::U32Le,
                "Rendering intent",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "profile_data",
                112,
                FieldType::U32Le,
                "Offset to ICC profile data",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "profile_size",
                116,
                FieldType::U32Le,
                "ICC profile size",
                true,
            );
            push_if_fits(
                &mut fields,
                present_len,
                "reserved",
                120,
                FieldType::U32Le,
                "Reserved field",
                true,
            );
        }
    }

    let parsed_len = if dib_size == CORE_HEADER_LEN {
        CORE_HEADER_LEN
    } else if dib_size >= V5_INFO_HEADER_LEN {
        V5_INFO_HEADER_LEN
    } else if dib_size >= V4_INFO_HEADER_LEN {
        V4_INFO_HEADER_LEN
    } else if dib_size >= V3_INFO_HEADER_LEN {
        V3_INFO_HEADER_LEN
    } else if dib_size >= V2_INFO_HEADER_LEN {
        V2_INFO_HEADER_LEN
    } else if dib_size >= INFO_HEADER_LEN {
        INFO_HEADER_LEN
    } else {
        4
    };

    if present_len > parsed_len {
        fields.push(FieldDef {
            name: "dib_extra".into(),
            offset: parsed_len,
            field_type: FieldType::DataRange(present_len - parsed_len),
            description: "Raw DIB header bytes beyond the parsed fields".into(),
            editable: false,
        });
    }

    StructDef {
        name: if present_len >= dib_size {
            dib_header_name(dib_size).into()
        } else {
            format!("{} (truncated)", dib_header_name(dib_size))
        },
        base_offset: BITMAP_FILE_HEADER_LEN,
        fields,
        children: vec![],
    }
}

fn push_if_fits(
    fields: &mut Vec<FieldDef>,
    present_len: u64,
    name: &str,
    offset: u64,
    field_type: FieldType,
    description: &str,
    editable: bool,
) {
    let Some(size) = field_type.byte_size() else {
        return;
    };
    if offset + size as u64 <= present_len {
        fields.push(FieldDef {
            name: name.into(),
            offset,
            field_type,
            description: description.into(),
            editable,
        });
    }
}

fn dib_header_name(dib_size: u64) -> &'static str {
    match dib_size {
        CORE_HEADER_LEN => "BITMAPCOREHEADER",
        INFO_HEADER_LEN => "BITMAPINFOHEADER",
        V2_INFO_HEADER_LEN => "BITMAPV2INFOHEADER",
        V3_INFO_HEADER_LEN => "BITMAPV3INFOHEADER",
        V4_INFO_HEADER_LEN => "BITMAPV4HEADER",
        V5_INFO_HEADER_LEN => "BITMAPV5HEADER",
        64 => "OS22XBITMAPHEADER",
        _ => "DIB Header",
    }
}

fn bounded_file_end(doc_len: u64, declared_file_size: u64, minimum: u64) -> u64 {
    if declared_file_size >= minimum && declared_file_size <= doc_len {
        declared_file_size
    } else {
        doc_len
    }
}

fn compression_variants() -> Vec<(u64, String)> {
    vec![
        (BI_RGB as u64, "BI_RGB".into()),
        (BI_RLE8 as u64, "BI_RLE8".into()),
        (BI_RLE4 as u64, "BI_RLE4".into()),
        (BI_BITFIELDS as u64, "BI_BITFIELDS".into()),
        (BI_JPEG as u64, "BI_JPEG".into()),
        (BI_PNG as u64, "BI_PNG".into()),
        (BI_ALPHABITFIELDS as u64, "BI_ALPHABITFIELDS".into()),
        (BI_CMYK as u64, "BI_CMYK".into()),
        (BI_CMYKRLE8 as u64, "BI_CMYKRLE8".into()),
        (BI_CMYKRLE4 as u64, "BI_CMYKRLE4".into()),
    ]
}

fn read_u16_le(doc: &mut Document, offset: u64) -> Option<u16> {
    let bytes = read_bytes_raw(doc, offset, 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(doc: &mut Document, offset: u64) -> Option<u32> {
    let bytes = read_bytes_raw(doc, offset, 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap, BI_BITFIELDS};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format;

    fn write_bmp(path: &std::path::Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    fn bitmap_file_header(file_size: u32, pixel_offset: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&file_size.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&pixel_offset.to_le_bytes());
        bytes
    }

    fn info_header(
        width: i32,
        height: i32,
        bits_per_pixel: u16,
        compression: u32,
        image_size: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&bits_per_pixel.to_le_bytes());
        bytes.extend_from_slice(&compression.to_le_bytes());
        bytes.extend_from_slice(&image_size.to_le_bytes());
        bytes.extend_from_slice(&2835_i32.to_le_bytes());
        bytes.extend_from_slice(&2835_i32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes
    }

    fn sample_bmp() -> Vec<u8> {
        let mut bytes = bitmap_file_header(58, 54);
        bytes.extend_from_slice(&info_header(1, 1, 24, 0, 4));
        bytes.extend_from_slice(&[0x00, 0x00, 0xff, 0x00]);
        bytes
    }

    fn sample_bmp_with_masks() -> Vec<u8> {
        let mut bytes = bitmap_file_header(70, 66);
        bytes.extend_from_slice(&info_header(1, 1, 32, BI_BITFIELDS, 4));
        bytes.extend_from_slice(&0x00ff_0000_u32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_ff00_u32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_00ff_u32.to_le_bytes());
        bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        bytes
    }

    #[test]
    fn detects_bitmap_headers_and_pixel_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.bmp");
        let mut doc = write_bmp(&path, &sample_bmp());

        let def = detect(&mut doc).expect("bmp detected");
        assert_eq!(def.name, "BMP");
        assert_eq!(def.structs[0].name, "Bitmap File Header");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "BITMAPINFOHEADER"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Pixel Data"));

        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
        let pixel_data = structs
            .iter()
            .find(|structure| structure.name == "Pixel Data")
            .expect("pixel data struct")
            .fields
            .iter()
            .find(|field| field.def.name == "pixel_data")
            .expect("pixel data field");
        assert_eq!(pixel_data.size, 4);
    }

    #[test]
    fn detects_external_bit_masks_after_info_header() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bitfields.bmp");
        let mut doc = write_bmp(&path, &sample_bmp_with_masks());

        let def = detect_with_cap(&mut doc, 64).expect("bmp detected");
        let masks = def
            .structs
            .iter()
            .find(|structure| structure.name == "Bit Masks")
            .expect("bit masks struct");
        assert_eq!(masks.fields.len(), 3);
    }

    #[test]
    fn keeps_detecting_truncated_bmp_after_magic_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("truncated.bmp");
        let mut bytes = sample_bmp();
        bytes.truncate(28);
        let mut doc = write_bmp(&path, &bytes);

        let def = detect(&mut doc).expect("bmp still detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("truncated")));
    }
}
