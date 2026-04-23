use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

const RIFF_HEADER_LEN: u64 = 12;
const CHUNK_HEADER_LEN: u64 = 8;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < RIFF_HEADER_LEN {
        return None;
    }

    let header = read_bytes_raw(doc, 0, RIFF_HEADER_LEN as usize)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }

    let riff_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as u64;
    let logical_file_end = bounded_riff_end(doc.len(), riff_size);

    let mut structs = vec![StructDef {
        name: "RIFF Header".into(),
        base_offset: 0,
        fields: vec![
            FieldDef {
                name: "chunk_id".into(),
                offset: 0,
                field_type: FieldType::Utf8(4),
                description: "RIFF container identifier".into(),
                editable: false,
            },
            FieldDef {
                name: "chunk_size".into(),
                offset: 4,
                field_type: FieldType::U32Le,
                description: "Container size excluding the first 8 bytes".into(),
                editable: true,
            },
            FieldDef {
                name: "format".into(),
                offset: 8,
                field_type: FieldType::Utf8(4),
                description: "RIFF form type".into(),
                editable: true,
            },
        ],
        children: vec![],
    }];

    let mut offset = RIFF_HEADER_LEN;
    let mut chunk_index = 0_usize;
    let mut more_remain = false;

    while offset.saturating_add(CHUNK_HEADER_LEN) <= logical_file_end && offset < doc.len() {
        if chunk_index >= entry_cap.max(1) {
            if offset.saturating_add(CHUNK_HEADER_LEN) <= logical_file_end {
                more_remain = true;
            }
            break;
        }

        let Some(header) = read_bytes_raw(doc, offset, CHUNK_HEADER_LEN as usize) else {
            break;
        };
        let chunk_id = String::from_utf8_lossy(&header[0..4]).into_owned();
        let chunk_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as u64;
        let parsed = parse_chunk(
            doc,
            offset,
            chunk_index,
            &chunk_id,
            chunk_size,
            logical_file_end,
        );
        offset = parsed.next_offset;
        structs.push(parsed.structure);
        chunk_index += 1;
        if !parsed.complete {
            break;
        }
    }

    if logical_file_end < doc.len() {
        structs.push(StructDef {
            name: "Trailing Data".into(),
            base_offset: logical_file_end,
            fields: vec![FieldDef {
                name: "trailing_bytes".into(),
                offset: 0,
                field_type: FieldType::DataRange(doc.len() - logical_file_end),
                description: "Bytes beyond the RIFF size declared by the header".into(),
                editable: false,
            }],
            children: vec![],
        });
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "… more WAV chunks beyond {} (use `:insp more` to load more)",
                chunk_index
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "WAV".to_string(),
        structs,
    })
}

struct ParsedChunk {
    structure: StructDef,
    next_offset: u64,
    complete: bool,
}

fn parse_chunk(
    doc: &mut Document,
    offset: u64,
    chunk_index: usize,
    chunk_id: &str,
    chunk_size: u64,
    logical_file_end: u64,
) -> ParsedChunk {
    let data_offset = offset + CHUNK_HEADER_LEN;
    let available_payload = doc
        .len()
        .saturating_sub(data_offset)
        .min(logical_file_end.saturating_sub(data_offset));
    let payload_len = chunk_size.min(available_payload);
    let pad_len = if chunk_size & 1 == 1 { 1 } else { 0 };
    let declared_next_offset = offset
        .saturating_add(CHUNK_HEADER_LEN)
        .saturating_add(chunk_size)
        .saturating_add(pad_len);
    let complete = declared_next_offset <= logical_file_end && declared_next_offset <= doc.len();

    let mut fields = vec![
        FieldDef {
            name: "chunk_id".into(),
            offset: 0,
            field_type: FieldType::Utf8(4),
            description: "Chunk FourCC".into(),
            editable: true,
        },
        FieldDef {
            name: "chunk_size".into(),
            offset: 4,
            field_type: FieldType::U32Le,
            description: "Chunk payload size in bytes".into(),
            editable: true,
        },
    ];

    match chunk_id {
        "fmt " => extend_fmt_chunk_fields(&mut fields, payload_len),
        "data" => {
            if payload_len > 0 {
                fields.push(FieldDef {
                    name: "sample_data".into(),
                    offset: CHUNK_HEADER_LEN,
                    field_type: FieldType::DataRange(payload_len),
                    description: "Raw PCM/compressed sample bytes".into(),
                    editable: false,
                });
            }
        }
        "fact" => {
            push_if_payload_fits(
                &mut fields,
                payload_len,
                "sample_length",
                8,
                FieldType::U32Le,
                "Decoded sample length for compressed formats",
                true,
            );
            if payload_len > 4 {
                fields.push(FieldDef {
                    name: "fact_data".into(),
                    offset: 12,
                    field_type: FieldType::DataRange(payload_len - 4),
                    description: "Additional fact chunk bytes".into(),
                    editable: false,
                });
            }
        }
        "LIST" => {
            push_if_payload_fits(
                &mut fields,
                payload_len,
                "list_type",
                8,
                FieldType::Utf8(4),
                "LIST subtype (for example INFO or adtl)",
                true,
            );
            if payload_len > 4 {
                fields.push(FieldDef {
                    name: "list_data".into(),
                    offset: 12,
                    field_type: FieldType::DataRange(payload_len - 4),
                    description: "LIST payload bytes after the subtype".into(),
                    editable: false,
                });
            }
        }
        _ => {
            if payload_len > 0 {
                fields.push(FieldDef {
                    name: "chunk_data".into(),
                    offset: CHUNK_HEADER_LEN,
                    field_type: FieldType::DataRange(payload_len),
                    description: "Raw chunk payload bytes".into(),
                    editable: false,
                });
            }
        }
    }

    if chunk_size & 1 == 1 && complete {
        fields.push(FieldDef {
            name: "padding_byte".into(),
            offset: CHUNK_HEADER_LEN + chunk_size,
            field_type: FieldType::U8,
            description: "Alignment byte following an odd-sized chunk".into(),
            editable: false,
        });
    }

    let display_name = chunk_display_name(doc, data_offset, chunk_id, payload_len);
    ParsedChunk {
        structure: StructDef {
            name: if complete {
                format!("Chunk {chunk_index}: {display_name}")
            } else {
                format!("Chunk {chunk_index}: {display_name} (truncated)")
            },
            base_offset: offset,
            fields,
            children: vec![],
        },
        next_offset: if complete {
            declared_next_offset
        } else {
            logical_file_end.min(doc.len())
        },
        complete,
    }
}

fn extend_fmt_chunk_fields(fields: &mut Vec<FieldDef>, payload_len: u64) {
    push_if_payload_fits(
        fields,
        payload_len,
        "audio_format",
        8,
        FieldType::Enum {
            inner: Box::new(FieldType::U16Le),
            variants: audio_format_variants(),
        },
        "WAVE format tag",
        true,
    );
    push_if_payload_fits(
        fields,
        payload_len,
        "num_channels",
        10,
        FieldType::U16Le,
        "Number of audio channels",
        true,
    );
    push_if_payload_fits(
        fields,
        payload_len,
        "sample_rate",
        12,
        FieldType::U32Le,
        "Samples per second",
        true,
    );
    push_if_payload_fits(
        fields,
        payload_len,
        "byte_rate",
        16,
        FieldType::U32Le,
        "Average bytes per second",
        true,
    );
    push_if_payload_fits(
        fields,
        payload_len,
        "block_align",
        20,
        FieldType::U16Le,
        "Bytes per sample frame",
        true,
    );
    push_if_payload_fits(
        fields,
        payload_len,
        "bits_per_sample",
        22,
        FieldType::U16Le,
        "Bits per sample",
        true,
    );
    push_if_payload_fits(
        fields,
        payload_len,
        "cb_size",
        24,
        FieldType::U16Le,
        "Extra format bytes count",
        true,
    );
    if payload_len > 18 {
        fields.push(FieldDef {
            name: "extra_format_data".into(),
            offset: 26,
            field_type: FieldType::DataRange(payload_len - 18),
            description: "Codec-specific WAVEFORMATEX payload".into(),
            editable: false,
        });
    }
}

fn push_if_payload_fits(
    fields: &mut Vec<FieldDef>,
    payload_len: u64,
    name: &str,
    offset: u64,
    field_type: FieldType,
    description: &str,
    editable: bool,
) {
    let Some(size) = field_type.byte_size() else {
        return;
    };
    if offset + size as u64 <= CHUNK_HEADER_LEN + payload_len {
        fields.push(FieldDef {
            name: name.into(),
            offset,
            field_type,
            description: description.into(),
            editable,
        });
    }
}

fn chunk_display_name(
    doc: &mut Document,
    data_offset: u64,
    chunk_id: &str,
    payload_len: u64,
) -> String {
    match chunk_id {
        "fmt " => {
            let audio_format = if payload_len >= 2 {
                read_u16_le(doc, data_offset).map(audio_format_name)
            } else {
                None
            };
            if let Some(audio_format) = audio_format {
                format!("fmt ({audio_format})")
            } else {
                "fmt".into()
            }
        }
        "LIST" => {
            if payload_len >= 4 {
                if let Some(list_type) = read_fourcc(doc, data_offset) {
                    return format!("LIST ({})", list_type.trim_end());
                }
            }
            "LIST".into()
        }
        other => other.trim_end().to_owned(),
    }
}

fn bounded_riff_end(doc_len: u64, riff_size: u64) -> u64 {
    let declared_end = 8_u64.saturating_add(riff_size);
    if declared_end >= RIFF_HEADER_LEN && declared_end <= doc_len {
        declared_end
    } else {
        doc_len
    }
}

fn audio_format_variants() -> Vec<(u64, String)> {
    vec![
        (0x0001, "PCM".into()),
        (0x0003, "IEEE_FLOAT".into()),
        (0x0006, "ALAW".into()),
        (0x0007, "MULAW".into()),
        (0xfffe, "EXTENSIBLE".into()),
    ]
}

fn audio_format_name(format: u16) -> String {
    audio_format_variants()
        .into_iter()
        .find_map(|(value, name)| (value == format as u64).then_some(name))
        .unwrap_or_else(|| format!("0x{format:04x}"))
}

fn read_u16_le(doc: &mut Document, offset: u64) -> Option<u16> {
    let bytes = read_bytes_raw(doc, offset, 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_fourcc(doc: &mut Document, offset: u64) -> Option<String> {
    let bytes = read_bytes_raw(doc, offset, 4)?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format;

    fn write_wav(path: &std::path::Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    fn riff_wave(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut payload = Vec::new();
        for (id, chunk) in chunks {
            payload.extend_from_slice(*id);
            payload.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            payload.extend_from_slice(chunk);
            if chunk.len() & 1 == 1 {
                payload.push(0);
            }
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(4 + payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn fmt_chunk() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_000_u32.to_le_bytes());
        bytes.extend_from_slice(&8_000_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes
    }

    fn sample_wav() -> Vec<u8> {
        riff_wave(&[
            (b"fmt ", fmt_chunk()),
            (b"data", vec![0x00, 0x7f, 0x80, 0xff]),
        ])
    }

    fn sample_wav_with_odd_chunk() -> Vec<u8> {
        riff_wave(&[
            (b"fmt ", fmt_chunk()),
            (b"JUNK", vec![0xaa, 0xbb, 0xcc]),
            (b"data", vec![0x01, 0x02]),
        ])
    }

    #[test]
    fn detects_fmt_and_data_chunks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.wav");
        let mut doc = write_wav(&path, &sample_wav());

        let def = detect(&mut doc).expect("wav detected");
        assert_eq!(def.name, "WAV");
        assert_eq!(def.structs[0].name, "RIFF Header");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("fmt (PCM)")));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("data")));

        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
        let data = structs
            .iter()
            .find(|structure| structure.name.contains("data"))
            .expect("data chunk")
            .fields
            .iter()
            .find(|field| field.def.name == "sample_data")
            .expect("sample data field");
        assert_eq!(data.size, 4);
    }

    #[test]
    fn paginates_and_keeps_padding_for_odd_sized_chunks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("odd.wav");
        let mut doc = write_wav(&path, &sample_wav_with_odd_chunk());

        let capped = detect_with_cap(&mut doc, 2).expect("wav detected");
        assert!(capped
            .structs
            .iter()
            .any(|structure| structure.name.contains("more WAV chunks beyond 2")));

        let full = detect_with_cap(&mut doc, 8).expect("wav detected");
        let junk = full
            .structs
            .iter()
            .find(|structure| structure.name.contains("JUNK"))
            .expect("junk chunk");
        assert!(junk.fields.iter().any(|field| field.name == "padding_byte"));
    }

    #[test]
    fn keeps_detecting_truncated_wav_after_magic_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("truncated.wav");
        let mut bytes = sample_wav();
        bytes.truncate(26);
        let mut doc = write_wav(&path, &bytes);

        let def = detect(&mut doc).expect("wav still detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("truncated")));
    }
}
