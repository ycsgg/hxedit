use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

const BLOCK_HEADER_LEN: u64 = 8;
const BLOCK_TRAILER_LEN: u64 = 4;
const MIN_BLOCK_LEN: u64 = BLOCK_HEADER_LEN + BLOCK_TRAILER_LEN;

const SHB_TYPE_BYTES: [u8; 4] = [0x0a, 0x0d, 0x0d, 0x0a];
const BYTE_ORDER_MAGIC_LE: [u8; 4] = [0x4d, 0x3c, 0x2b, 0x1a];
const BYTE_ORDER_MAGIC_BE: [u8; 4] = [0x1a, 0x2b, 0x3c, 0x4d];

const BLOCK_TYPE_SECTION_HEADER: u32 = 0x0a0d0d0a;
const BLOCK_TYPE_INTERFACE_DESCRIPTION: u32 = 0x0000_0001;
const BLOCK_TYPE_OBSOLETE_PACKET: u32 = 0x0000_0002;
const BLOCK_TYPE_SIMPLE_PACKET: u32 = 0x0000_0003;
const BLOCK_TYPE_NAME_RESOLUTION: u32 = 0x0000_0004;
const BLOCK_TYPE_INTERFACE_STATISTICS: u32 = 0x0000_0005;
const BLOCK_TYPE_ENHANCED_PACKET: u32 = 0x0000_0006;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < 12 {
        return None;
    }

    let first = read_bytes_raw(doc, 0, 12)?;
    if first[0..4] != SHB_TYPE_BYTES {
        return None;
    }
    let mut endian = Endian::from_bom(&first[8..12])?;
    let first_total_len = endian.read_u32(&first, 4) as u64;
    if first_total_len < 28 {
        return None;
    }

    let mut structs = Vec::new();
    let mut offset = 0_u64;
    let mut block_index = 0_usize;
    let mut more_remain = false;
    while offset.saturating_add(MIN_BLOCK_LEN) <= doc.len() {
        if block_index >= entry_cap.max(1) {
            more_remain = true;
            break;
        }

        let Some(parsed) = parse_block(doc, offset, block_index, endian) else {
            break;
        };
        if let Some(new_endian) = parsed.new_section_endian {
            endian = new_endian;
        }
        offset = parsed.next_offset;
        let complete = parsed.complete;
        structs.push(parsed.structure);
        block_index += 1;
        if !complete {
            break;
        }
    }

    if structs.is_empty() {
        return None;
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "... more PCAPNG blocks beyond {} (use `:insp more` to load more)",
                block_index
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "PCAPNG".to_string(),
        structs,
    })
}

struct ParsedBlock {
    structure: StructDef,
    next_offset: u64,
    complete: bool,
    new_section_endian: Option<Endian>,
}

fn parse_block(
    doc: &mut Document,
    offset: u64,
    block_index: usize,
    current_endian: Endian,
) -> Option<ParsedBlock> {
    let header = read_bytes_raw(doc, offset, BLOCK_HEADER_LEN as usize)?;
    let is_section = header[0..4] == SHB_TYPE_BYTES;
    let endian = if is_section {
        let bom = read_bytes_raw(doc, offset + 8, 4)?;
        Endian::from_bom(&bom)?
    } else {
        current_endian
    };
    let block_type = if is_section {
        BLOCK_TYPE_SECTION_HEADER
    } else {
        endian.read_u32(&header, 0)
    };
    let total_len = endian.read_u32(&header, 4) as u64;

    if total_len < MIN_BLOCK_LEN || !total_len.is_multiple_of(4) {
        return Some(truncated_block(
            doc,
            offset,
            block_index,
            endian,
            block_type,
            "invalid length",
        ));
    }

    let complete = offset
        .checked_add(total_len)
        .is_some_and(|end| end <= doc.len());
    if !complete {
        return Some(truncated_block(
            doc,
            offset,
            block_index,
            endian,
            block_type,
            "truncated",
        ));
    }

    let structure = match block_type {
        BLOCK_TYPE_SECTION_HEADER => section_header_block(offset, total_len, endian),
        BLOCK_TYPE_INTERFACE_DESCRIPTION => {
            interface_description_block(doc, offset, total_len, endian)
        }
        BLOCK_TYPE_ENHANCED_PACKET => enhanced_packet_block(doc, offset, total_len, endian),
        BLOCK_TYPE_SIMPLE_PACKET => simple_packet_block(doc, offset, total_len, endian),
        _ => generic_block(block_index, offset, total_len, endian, block_type),
    };

    Some(ParsedBlock {
        structure,
        next_offset: offset + total_len,
        complete: true,
        new_section_endian: is_section.then_some(endian),
    })
}

fn truncated_block(
    doc: &mut Document,
    offset: u64,
    block_index: usize,
    endian: Endian,
    block_type: u32,
    reason: &str,
) -> ParsedBlock {
    let available_len = doc.len().saturating_sub(offset);
    let mut fields = vec![
        field(
            "block_type",
            0,
            block_type_type(endian, block_type),
            "PCAPNG block type",
            false,
        ),
        field(
            "block_total_length",
            4,
            endian.u32_type(),
            "Declared block total length",
            false,
        ),
    ];
    if available_len > BLOCK_HEADER_LEN {
        fields.push(field(
            "available_block_bytes",
            BLOCK_HEADER_LEN,
            FieldType::DataRange(available_len - BLOCK_HEADER_LEN),
            "Available bytes after the block header",
            false,
        ));
    }

    ParsedBlock {
        structure: StructDef {
            name: format!(
                "PCAPNG Block {block_index}: {} ({reason})",
                block_type_label(block_type)
            ),
            base_offset: offset,
            fields,
            children: vec![],
        },
        next_offset: doc.len(),
        complete: false,
        new_section_endian: None,
    }
}

fn section_header_block(offset: u64, total_len: u64, endian: Endian) -> StructDef {
    let mut fields = common_block_fields(endian, BLOCK_TYPE_SECTION_HEADER);
    fields.extend([
        field(
            "byte_order_magic",
            8,
            FieldType::Bytes(4),
            "Section byte-order magic",
            false,
        ),
        field(
            "major_version",
            12,
            endian.u16_type(),
            "PCAPNG section major version",
            false,
        ),
        field(
            "minor_version",
            14,
            endian.u16_type(),
            "PCAPNG section minor version",
            false,
        ),
        field(
            "section_length",
            16,
            endian.i64_type(),
            "Section length, or -1 when unknown",
            false,
        ),
    ]);
    add_options_and_trailer(&mut fields, total_len, 24, endian);

    StructDef {
        name: format!("Section Header Block ({})", endian.label()),
        base_offset: offset,
        fields,
        children: vec![],
    }
}

fn interface_description_block(
    doc: &mut Document,
    offset: u64,
    total_len: u64,
    endian: Endian,
) -> StructDef {
    let mut fields = common_block_fields(endian, BLOCK_TYPE_INTERFACE_DESCRIPTION);
    fields.extend([
        field(
            "linktype",
            8,
            FieldType::custom_enum(endian.u16_type(), linktype_variants()),
            "Link-layer type for packets on this interface",
            false,
        ),
        field("reserved", 10, endian.u16_type(), "Reserved field", false),
        field(
            "snaplen",
            12,
            endian.u32_type(),
            "Maximum captured packet length for this interface",
            false,
        ),
    ]);
    add_options_and_trailer(&mut fields, total_len, 16, endian);

    StructDef {
        name: block_name(
            doc,
            offset,
            total_len,
            endian,
            "Interface Description Block",
        ),
        base_offset: offset,
        fields,
        children: vec![],
    }
}

fn enhanced_packet_block(
    doc: &mut Document,
    offset: u64,
    total_len: u64,
    endian: Endian,
) -> StructDef {
    let mut fields = common_block_fields(endian, BLOCK_TYPE_ENHANCED_PACKET);
    fields.extend([
        field(
            "interface_id",
            8,
            endian.u32_type(),
            "Interface description block index",
            false,
        ),
        field(
            "timestamp_high",
            12,
            endian.u32_type(),
            "High 32 bits of the packet timestamp",
            false,
        ),
        field(
            "timestamp_low",
            16,
            endian.u32_type(),
            "Low 32 bits of the packet timestamp",
            false,
        ),
        field(
            "captured_len",
            20,
            endian.u32_type(),
            "Captured packet byte length",
            false,
        ),
        field(
            "original_len",
            24,
            endian.u32_type(),
            "Original packet byte length on the wire",
            false,
        ),
    ]);

    let captured_len = read_u32_at(doc, offset + 20, endian).unwrap_or(0) as u64;
    let data_offset = 28_u64;
    let padded_data_len = pad4(captured_len);
    let data_end_offset = total_len.saturating_sub(BLOCK_TRAILER_LEN);
    if captured_len > 0 && data_offset + captured_len <= data_end_offset {
        fields.push(field(
            "packet_data",
            data_offset,
            FieldType::DataRange(captured_len),
            "Captured packet bytes; link-layer payload is not decoded",
            false,
        ));
    }
    let options_offset = data_offset.saturating_add(padded_data_len);
    add_options_and_trailer(&mut fields, total_len, options_offset, endian);

    StructDef {
        name: block_name(doc, offset, total_len, endian, "Enhanced Packet Block"),
        base_offset: offset,
        fields,
        children: vec![],
    }
}

fn simple_packet_block(
    doc: &mut Document,
    offset: u64,
    total_len: u64,
    endian: Endian,
) -> StructDef {
    let mut fields = common_block_fields(endian, BLOCK_TYPE_SIMPLE_PACKET);
    fields.push(field(
        "original_len",
        8,
        endian.u32_type(),
        "Original packet byte length on the wire",
        false,
    ));

    let body_len = total_len.saturating_sub(MIN_BLOCK_LEN);
    let data_len = body_len.saturating_sub(4);
    if data_len > 0 {
        fields.push(field(
            "packet_data",
            12,
            FieldType::DataRange(data_len),
            "Packet bytes carried by the simple packet block",
            false,
        ));
    }
    fields.push(field(
        "trailing_total_length",
        total_len - BLOCK_TRAILER_LEN,
        endian.u32_type(),
        "Repeated block total length",
        false,
    ));

    StructDef {
        name: block_name(doc, offset, total_len, endian, "Simple Packet Block"),
        base_offset: offset,
        fields,
        children: vec![],
    }
}

fn generic_block(
    block_index: usize,
    offset: u64,
    total_len: u64,
    endian: Endian,
    block_type: u32,
) -> StructDef {
    let mut fields = common_block_fields(endian, block_type);
    let body_len = total_len.saturating_sub(MIN_BLOCK_LEN);
    if body_len > 0 {
        fields.push(field(
            "block_body",
            BLOCK_HEADER_LEN,
            FieldType::DataRange(body_len),
            "Raw PCAPNG block body",
            false,
        ));
    }
    fields.push(field(
        "trailing_total_length",
        total_len - BLOCK_TRAILER_LEN,
        endian.u32_type(),
        "Repeated block total length",
        false,
    ));

    StructDef {
        name: format!(
            "PCAPNG Block {block_index}: {}",
            block_type_label(block_type)
        ),
        base_offset: offset,
        fields,
        children: vec![],
    }
}

fn common_block_fields(endian: Endian, block_type: u32) -> Vec<FieldDef> {
    vec![
        field(
            "block_type",
            0,
            block_type_type(endian, block_type),
            "PCAPNG block type",
            false,
        ),
        field(
            "block_total_length",
            4,
            endian.u32_type(),
            "Block total length including header and trailing length",
            false,
        ),
    ]
}

fn add_options_and_trailer(
    fields: &mut Vec<FieldDef>,
    total_len: u64,
    options_offset: u64,
    endian: Endian,
) {
    let body_end_offset = total_len.saturating_sub(BLOCK_TRAILER_LEN);
    if options_offset < body_end_offset {
        fields.push(field(
            "options",
            options_offset,
            FieldType::DataRange(body_end_offset - options_offset),
            "Raw block options",
            false,
        ));
    }
    fields.push(field(
        "trailing_total_length",
        total_len - BLOCK_TRAILER_LEN,
        endian.u32_type(),
        "Repeated block total length",
        false,
    ));
}

fn block_name(
    doc: &mut Document,
    offset: u64,
    total_len: u64,
    endian: Endian,
    fallback: &str,
) -> String {
    let Some(block_type) = read_u32_at(doc, offset, endian) else {
        return fallback.to_owned();
    };
    if total_len < MIN_BLOCK_LEN {
        format!("{fallback} (invalid length)")
    } else {
        block_type_label(block_type).to_owned()
    }
}

fn block_type_type(endian: Endian, block_type: u32) -> FieldType {
    FieldType::custom_enum(
        endian.u32_type(),
        vec![(block_type as u64, block_type_label(block_type).into())],
    )
}

fn block_type_label(block_type: u32) -> &'static str {
    match block_type {
        BLOCK_TYPE_SECTION_HEADER => "Section Header Block",
        BLOCK_TYPE_INTERFACE_DESCRIPTION => "Interface Description Block",
        BLOCK_TYPE_OBSOLETE_PACKET => "Obsolete Packet Block",
        BLOCK_TYPE_SIMPLE_PACKET => "Simple Packet Block",
        BLOCK_TYPE_NAME_RESOLUTION => "Name Resolution Block",
        BLOCK_TYPE_INTERFACE_STATISTICS => "Interface Statistics Block",
        BLOCK_TYPE_ENHANCED_PACKET => "Enhanced Packet Block",
        _ => "Unknown Block",
    }
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn from_bom(bytes: &[u8]) -> Option<Self> {
        if bytes == BYTE_ORDER_MAGIC_LE {
            Some(Self::Little)
        } else if bytes == BYTE_ORDER_MAGIC_BE {
            Some(Self::Big)
        } else {
            None
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Little => "little endian",
            Self::Big => "big endian",
        }
    }

    const fn u16_type(self) -> FieldType {
        match self {
            Self::Little => FieldType::U16Le,
            Self::Big => FieldType::U16Be,
        }
    }

    const fn u32_type(self) -> FieldType {
        match self {
            Self::Little => FieldType::U32Le,
            Self::Big => FieldType::U32Be,
        }
    }

    const fn i64_type(self) -> FieldType {
        match self {
            Self::Little => FieldType::I64Le,
            Self::Big => FieldType::I64Be,
        }
    }

    fn read_u32(self, bytes: &[u8], offset: usize) -> u32 {
        let raw = [
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ];
        match self {
            Self::Little => u32::from_le_bytes(raw),
            Self::Big => u32::from_be_bytes(raw),
        }
    }
}

fn read_u32_at(doc: &mut Document, offset: u64, endian: Endian) -> Option<u32> {
    let bytes = read_bytes_raw(doc, offset, 4)?;
    Some(endian.read_u32(&bytes, 0))
}

const fn pad4(len: u64) -> u64 {
    (len + 3) & !3
}

fn linktype_variants() -> Vec<(u64, String)> {
    vec![
        (0, "Null/loopback".into()),
        (1, "Ethernet".into()),
        (101, "Raw IP".into()),
        (105, "IEEE 802.11".into()),
        (113, "Linux cooked capture".into()),
        (127, "Radiotap".into()),
        (228, "IPv4".into()),
        (229, "IPv6".into()),
        (276, "Linux cooked capture v2".into()),
    ]
}

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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format::types::FieldType;

    fn open_pcapng(bytes: Vec<u8>) -> Document {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.pcapng");
        fs::write(&path, bytes).unwrap();
        Document::open(&path, &Config::default()).unwrap()
    }

    fn le_block(block_type: u32, body: &[u8]) -> Vec<u8> {
        let total_len = (12 + body.len()) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&block_type.to_le_bytes());
        bytes.extend_from_slice(&total_len.to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&total_len.to_le_bytes());
        bytes
    }

    fn le_sample() -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut shb_body = Vec::new();
        shb_body.extend_from_slice(&super::BYTE_ORDER_MAGIC_LE);
        shb_body.extend_from_slice(&1_u16.to_le_bytes());
        shb_body.extend_from_slice(&0_u16.to_le_bytes());
        shb_body.extend_from_slice(&(-1_i64).to_le_bytes());
        bytes.extend(le_block(super::BLOCK_TYPE_SECTION_HEADER, &shb_body));

        let mut idb_body = Vec::new();
        idb_body.extend_from_slice(&1_u16.to_le_bytes());
        idb_body.extend_from_slice(&0_u16.to_le_bytes());
        idb_body.extend_from_slice(&65_535_u32.to_le_bytes());
        bytes.extend(le_block(super::BLOCK_TYPE_INTERFACE_DESCRIPTION, &idb_body));

        let packet = [0xaa, 0xbb, 0xcc];
        let mut epb_body = Vec::new();
        epb_body.extend_from_slice(&0_u32.to_le_bytes());
        epb_body.extend_from_slice(&0_u32.to_le_bytes());
        epb_body.extend_from_slice(&42_u32.to_le_bytes());
        epb_body.extend_from_slice(&(packet.len() as u32).to_le_bytes());
        epb_body.extend_from_slice(&(packet.len() as u32).to_le_bytes());
        epb_body.extend_from_slice(&packet);
        epb_body.push(0);
        bytes.extend(le_block(super::BLOCK_TYPE_ENHANCED_PACKET, &epb_body));

        bytes
    }

    #[test]
    fn parses_little_endian_pcapng_section_interface_and_packet() {
        let mut doc = open_pcapng(le_sample());
        let def = detect(&mut doc).expect("pcapng should be detected");

        assert_eq!(def.name, "PCAPNG");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Section Header Block (little endian)"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Interface Description Block"));
        let packet = def
            .structs
            .iter()
            .find(|structure| structure.name == "Enhanced Packet Block")
            .expect("enhanced packet");
        assert!(packet.fields.iter().any(|field| field.name == "packet_data"
            && matches!(field.field_type, FieldType::DataRange(3))));
    }

    #[test]
    fn paginates_pcapng_blocks_and_parses_simple_packet() {
        let mut bytes = le_sample();
        let mut spb_body = Vec::new();
        spb_body.extend_from_slice(&2_u32.to_le_bytes());
        spb_body.extend_from_slice(&[0x11, 0x22, 0, 0]);
        bytes.extend(le_block(super::BLOCK_TYPE_SIMPLE_PACKET, &spb_body));

        let mut doc = open_pcapng(bytes);
        let def = detect_with_cap(&mut doc, 2).expect("pcapng should be detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("more PCAPNG blocks beyond 2")));

        let mut doc = open_pcapng({
            let mut full = le_sample();
            let mut body = Vec::new();
            body.extend_from_slice(&2_u32.to_le_bytes());
            body.extend_from_slice(&[0x11, 0x22, 0, 0]);
            full.extend(le_block(super::BLOCK_TYPE_SIMPLE_PACKET, &body));
            full
        });
        let def = detect(&mut doc).expect("pcapng should be detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Simple Packet Block"));
    }

    #[test]
    fn parses_big_endian_section_header() {
        let mut body = Vec::new();
        body.extend_from_slice(&super::BYTE_ORDER_MAGIC_BE);
        body.extend_from_slice(&1_u16.to_be_bytes());
        body.extend_from_slice(&0_u16.to_be_bytes());
        body.extend_from_slice(&(-1_i64).to_be_bytes());
        let total_len = (12 + body.len()) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&super::SHB_TYPE_BYTES);
        bytes.extend_from_slice(&total_len.to_be_bytes());
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(&total_len.to_be_bytes());

        let mut doc = open_pcapng(bytes);
        let def = detect(&mut doc).expect("pcapng should be detected");
        assert_eq!(def.structs[0].name, "Section Header Block (big endian)");
    }
}
