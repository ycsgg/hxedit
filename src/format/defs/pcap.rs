use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::time;
use crate::format::types::*;

const PCAP_HEADER_LEN: u64 = 24;
const PCAP_PACKET_HEADER_LEN: u64 = 16;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < PCAP_HEADER_LEN {
        return None;
    }

    let magic = read_bytes_raw(doc, 0, 4)?;
    let (endian, precision) = PcapMagic::parse(&magic)?;
    let mut structs = vec![global_header_struct(endian, precision)];

    let mut offset = PCAP_HEADER_LEN;
    let mut packet_index = 0_usize;
    let mut more_remain = false;
    while offset.saturating_add(PCAP_PACKET_HEADER_LEN) <= doc.len() {
        if packet_index >= entry_cap.max(1) {
            more_remain = true;
            break;
        }

        let Some(packet) = packet_struct(doc, offset, packet_index, endian, precision) else {
            break;
        };
        offset = packet.next_offset;
        let complete = packet.complete;
        structs.push(packet.structure);
        packet_index += 1;
        if !complete {
            break;
        }
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "... more PCAP packets beyond {} (use `:insp more` to load more)",
                packet_index
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "PCAP".to_string(),
        structs,
    })
}

fn global_header_struct(endian: Endian, precision: TimestampPrecision) -> StructDef {
    StructDef {
        name: format!("PCAP Global Header ({})", precision.label()),
        base_offset: 0,
        fields: vec![
            field(
                "magic",
                0,
                FieldType::Bytes(4),
                "PCAP magic number; determines byte order and timestamp precision",
                false,
            ),
            field(
                "version_major",
                4,
                endian.u16_type(),
                "PCAP major version",
                true,
            ),
            field(
                "version_minor",
                6,
                endian.u16_type(),
                "PCAP minor version",
                true,
            ),
            field(
                "thiszone",
                8,
                endian.i32_type(),
                "GMT to local correction, usually zero",
                true,
            ),
            field(
                "sigfigs",
                12,
                endian.u32_type(),
                "Timestamp accuracy, usually zero",
                true,
            ),
            field(
                "snaplen",
                16,
                endian.u32_type(),
                "Maximum bytes captured per packet",
                true,
            ),
            field(
                "network",
                20,
                FieldType::custom_enum(endian.u32_type(), linktype_variants()),
                "Link-layer header type for every packet record",
                true,
            ),
        ],
        children: vec![],
    }
}

struct ParsedPacket {
    structure: StructDef,
    next_offset: u64,
    complete: bool,
}

fn packet_struct(
    doc: &mut Document,
    offset: u64,
    packet_index: usize,
    endian: Endian,
    precision: TimestampPrecision,
) -> Option<ParsedPacket> {
    let header = read_bytes_raw(doc, offset, PCAP_PACKET_HEADER_LEN as usize)?;
    let incl_len = endian.read_u32(&header, 8) as u64;
    let packet_data_offset = offset + PCAP_PACKET_HEADER_LEN;
    let declared_end = packet_data_offset.checked_add(incl_len)?;
    let available_len = doc.len().saturating_sub(packet_data_offset).min(incl_len);
    let complete = declared_end <= doc.len();
    let ts_sec = endian.read_u32(&header, 0);
    let fraction = endian.read_u32(&header, 4);

    let mut fields = vec![
        field(
            "timestamp_utc",
            0,
            timestamp_type(endian, precision, ts_sec, fraction),
            "Packet timestamp as UTC; editing rewrites ts_sec and fractional timestamp together",
            true,
        ),
        field(
            "ts_sec",
            0,
            endian.u32_type(),
            "Packet timestamp seconds",
            true,
        ),
        field(
            precision.fraction_field_name(),
            4,
            endian.u32_type(),
            precision.fraction_description(),
            true,
        ),
        field(
            "incl_len",
            8,
            endian.u32_type(),
            "Captured packet byte length",
            true,
        ),
        field(
            "orig_len",
            12,
            endian.u32_type(),
            "Original packet byte length on the wire",
            true,
        ),
    ];

    if available_len > 0 {
        fields.push(field(
            "packet_data",
            PCAP_PACKET_HEADER_LEN,
            FieldType::DataRange(available_len),
            "Captured packet bytes; link-layer payload is not decoded",
            false,
        ));
    }

    Some(ParsedPacket {
        structure: StructDef {
            name: if complete {
                format!("Packet {packet_index}")
            } else {
                format!("Packet {packet_index} (truncated)")
            },
            base_offset: offset,
            fields,
            children: vec![],
        },
        next_offset: if complete { declared_end } else { doc.len() },
        complete,
    })
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
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

    const fn i32_type(self) -> FieldType {
        match self {
            Self::Little => FieldType::I32Le,
            Self::Big => FieldType::I32Be,
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

fn timestamp_type(
    endian: Endian,
    precision: TimestampPrecision,
    ts_sec: u32,
    fraction: u32,
) -> FieldType {
    FieldType::custom_display(
        8,
        time::format_unix_utc_fraction(
            ts_sec,
            fraction,
            precision.fraction_precision(),
            precision.fraction_field_name(),
        ),
        match (endian, precision) {
            (Endian::Little, TimestampPrecision::Microseconds) => encode_le_micro_timestamp,
            (Endian::Big, TimestampPrecision::Microseconds) => encode_be_micro_timestamp,
            (Endian::Little, TimestampPrecision::Nanoseconds) => encode_le_nano_timestamp,
            (Endian::Big, TimestampPrecision::Nanoseconds) => encode_be_nano_timestamp,
        },
    )
}

fn encode_le_micro_timestamp(input: &str) -> Result<Vec<u8>, String> {
    encode_timestamp(input, Endian::Little, TimestampPrecision::Microseconds)
}

fn encode_be_micro_timestamp(input: &str) -> Result<Vec<u8>, String> {
    encode_timestamp(input, Endian::Big, TimestampPrecision::Microseconds)
}

fn encode_le_nano_timestamp(input: &str) -> Result<Vec<u8>, String> {
    encode_timestamp(input, Endian::Little, TimestampPrecision::Nanoseconds)
}

fn encode_be_nano_timestamp(input: &str) -> Result<Vec<u8>, String> {
    encode_timestamp(input, Endian::Big, TimestampPrecision::Nanoseconds)
}

fn encode_timestamp(
    input: &str,
    endian: Endian,
    precision: TimestampPrecision,
) -> Result<Vec<u8>, String> {
    let (ts_sec, fraction) = parse_timestamp(input, precision)?;
    let mut bytes = Vec::with_capacity(8);
    match endian {
        Endian::Little => {
            bytes.extend_from_slice(&ts_sec.to_le_bytes());
            bytes.extend_from_slice(&fraction.to_le_bytes());
        }
        Endian::Big => {
            bytes.extend_from_slice(&ts_sec.to_be_bytes());
            bytes.extend_from_slice(&fraction.to_be_bytes());
        }
    }
    Ok(bytes)
}

fn parse_timestamp(input: &str, precision: TimestampPrecision) -> Result<(u32, u32), String> {
    time::parse_unix_utc_fraction(input, precision.fraction_precision())
}

#[derive(Clone, Copy)]
enum TimestampPrecision {
    Microseconds,
    Nanoseconds,
}

impl TimestampPrecision {
    const fn label(self) -> &'static str {
        match self {
            Self::Microseconds => "microsecond timestamps",
            Self::Nanoseconds => "nanosecond timestamps",
        }
    }

    const fn fraction_field_name(self) -> &'static str {
        match self {
            Self::Microseconds => "ts_usec",
            Self::Nanoseconds => "ts_nsec",
        }
    }

    const fn fraction_description(self) -> &'static str {
        match self {
            Self::Microseconds => "Packet timestamp microseconds",
            Self::Nanoseconds => "Packet timestamp nanoseconds",
        }
    }

    const fn fraction_precision(self) -> time::FractionPrecision {
        match self {
            Self::Microseconds => time::MICROSECOND,
            Self::Nanoseconds => time::NANOSECOND,
        }
    }
}

struct PcapMagic;

impl PcapMagic {
    fn parse(bytes: &[u8]) -> Option<(Endian, TimestampPrecision)> {
        match bytes {
            [0xa1, 0xb2, 0xc3, 0xd4] => Some((Endian::Big, TimestampPrecision::Microseconds)),
            [0xd4, 0xc3, 0xb2, 0xa1] => Some((Endian::Little, TimestampPrecision::Microseconds)),
            [0xa1, 0xb2, 0x3c, 0x4d] => Some((Endian::Big, TimestampPrecision::Nanoseconds)),
            [0x4d, 0x3c, 0xb2, 0xa1] => Some((Endian::Little, TimestampPrecision::Nanoseconds)),
            _ => None,
        }
    }
}

fn linktype_variants() -> Vec<(u64, String)> {
    vec![
        (0, "Null/loopback".into()),
        (1, "Ethernet".into()),
        (6, "IEEE 802.5 Token Ring".into()),
        (7, "ARCNET".into()),
        (8, "SLIP".into()),
        (9, "PPP".into()),
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
    use crate::format::edit::encode_value;
    use crate::format::types::FieldType;

    fn open_pcap(bytes: Vec<u8>) -> Document {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.pcap");
        fs::write(&path, bytes).unwrap();
        Document::open(&path, &Config::default()).unwrap()
    }

    fn little_pcap(packet_lengths: &[usize]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&65_535_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        for (index, &len) in packet_lengths.iter().enumerate() {
            bytes.extend_from_slice(&(1000 + index as u32).to_le_bytes());
            bytes.extend_from_slice(&123_u32.to_le_bytes());
            bytes.extend_from_slice(&(len as u32).to_le_bytes());
            bytes.extend_from_slice(&(len as u32).to_le_bytes());
            bytes.extend(std::iter::repeat_n(index as u8, len));
        }
        bytes
    }

    #[test]
    fn parses_little_endian_pcap_packets() {
        let mut doc = open_pcap(little_pcap(&[4, 2]));
        let def = detect(&mut doc).expect("pcap should be detected");

        assert_eq!(def.name, "PCAP");
        assert_eq!(
            def.structs[0].name,
            "PCAP Global Header (microsecond timestamps)"
        );
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Packet 1"));
        let packet = def
            .structs
            .iter()
            .find(|structure| structure.name == "Packet 0")
            .expect("packet 0");
        let timestamp = packet
            .fields
            .iter()
            .find(|field| field.name == "timestamp_utc")
            .expect("timestamp field");
        let FieldType::Custom(custom) = &timestamp.field_type else {
            panic!("timestamp should use custom formatting");
        };
        let crate::format::types::CustomCodec::Display { display, .. } = &custom.codec else {
            panic!("timestamp should use custom display codec");
        };
        assert_eq!(custom.bytes, 8);
        assert_eq!(display, "1970-01-01T00:16:40.000123Z");
        let mut expected_timestamp = Vec::new();
        expected_timestamp.extend_from_slice(&1000_u32.to_le_bytes());
        expected_timestamp.extend_from_slice(&123_u32.to_le_bytes());
        assert_eq!(
            encode_value(&timestamp.field_type, display).unwrap(),
            expected_timestamp
        );
        assert_eq!(
            encode_value(&timestamp.field_type, "1000.000123").unwrap(),
            expected_timestamp
        );
        assert!(packet.fields.iter().any(|field| field.name == "packet_data"
            && matches!(field.field_type, FieldType::DataRange(4))));
    }

    #[test]
    fn parses_big_endian_nanosecond_pcap_and_paginates() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0xa1, 0xb2, 0x3c, 0x4d]);
        bytes.extend_from_slice(&2_u16.to_be_bytes());
        bytes.extend_from_slice(&4_u16.to_be_bytes());
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&65_535_u32.to_be_bytes());
        bytes.extend_from_slice(&101_u32.to_be_bytes());
        for index in 0..2_u32 {
            bytes.extend_from_slice(&(2000 + index).to_be_bytes());
            bytes.extend_from_slice(&999_u32.to_be_bytes());
            bytes.extend_from_slice(&1_u32.to_be_bytes());
            bytes.extend_from_slice(&1_u32.to_be_bytes());
            bytes.push(index as u8);
        }

        let mut doc = open_pcap(bytes);
        let def = detect_with_cap(&mut doc, 1).expect("pcap should be detected");
        assert_eq!(
            def.structs[0].name,
            "PCAP Global Header (nanosecond timestamps)"
        );
        let packet = def
            .structs
            .iter()
            .find(|structure| structure.name == "Packet 0")
            .expect("packet 0");
        let timestamp = packet
            .fields
            .iter()
            .find(|field| field.name == "timestamp_utc")
            .expect("timestamp field");
        let FieldType::Custom(custom) = &timestamp.field_type else {
            panic!("timestamp should use custom formatting");
        };
        let crate::format::types::CustomCodec::Display { display, .. } = &custom.codec else {
            panic!("timestamp should use custom display codec");
        };
        assert_eq!(display, "1970-01-01T00:33:20.000000999Z");
        let mut expected_timestamp = Vec::new();
        expected_timestamp.extend_from_slice(&2000_u32.to_be_bytes());
        expected_timestamp.extend_from_slice(&999_u32.to_be_bytes());
        assert_eq!(
            encode_value(&timestamp.field_type, display).unwrap(),
            expected_timestamp
        );
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("more PCAP packets beyond 1")));
    }

    #[test]
    fn pcap_timestamp_rejects_fraction_beyond_precision() {
        assert!(
            super::encode_le_micro_timestamp("1970-01-01T00:00:00.1234567Z").is_err(),
            "microsecond PCAP timestamps should reject more than 6 fraction digits"
        );
        assert!(
            super::encode_le_nano_timestamp("1970-01-01T00:00:00.1234567890Z").is_err(),
            "nanosecond PCAP timestamps should reject more than 9 fraction digits"
        );
    }

    #[test]
    fn marks_truncated_pcap_packet() {
        let mut bytes = little_pcap(&[]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&[0xaa, 0xbb]);

        let mut doc = open_pcap(bytes);
        let def = detect(&mut doc).expect("pcap should be detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Packet 0 (truncated)"));
    }
}
