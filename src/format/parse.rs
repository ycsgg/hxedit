use crate::core::document::{ByteSlot, Document};
use crate::error::HxResult;
use crate::format::types::*;

/// A parsed field value.
///
/// Contains definition info, raw bytes read from the file, and formatted display text.
#[derive(Debug, Clone)]
pub struct FieldValue {
    /// The field definition this value corresponds to.
    pub def: FieldDef,
    /// Absolute offset of this field in the file (base_offset + def.offset).
    pub abs_offset: u64,
    /// Raw bytes read from the file (length == def.field_type.byte_size()).
    pub raw_bytes: Vec<u8>,
    /// Formatted display text, e.g. "0x003e (EM_X86_64)".
    pub display: String,
    /// Number of bytes this field occupies.
    pub size: usize,
}

/// A parsed structure block.
#[derive(Debug, Clone)]
pub struct StructValue {
    /// Structure name.
    pub name: String,
    /// Absolute start offset.
    pub base_offset: u64,
    /// Parsed field values.
    pub fields: Vec<FieldValue>,
    /// Parsed child structures.
    pub children: Vec<StructValue>,
}

/// A single row in the inspector panel.
///
/// Flattened from the StructValue tree for rendering.
#[derive(Debug, Clone)]
pub enum InspectorRow {
    /// Structure header line, e.g. "── ELF Header ────".
    Header { name: String, depth: usize },
    /// Field line.
    Field {
        /// Global index in the StructValue tree, used for locating during edits.
        field_index: usize,
        /// Field name.
        name: String,
        /// Formatted value.
        display: String,
        /// Absolute file offset range [start, start+size).
        abs_offset: u64,
        size: usize,
        /// Indentation depth.
        depth: usize,
        /// Whether this field is editable.
        editable: bool,
    },
}

/// Flatten a StructValue tree into a list of renderable rows.
pub fn flatten(structs: &[StructValue]) -> Vec<InspectorRow> {
    fn walk(sv: &StructValue, depth: usize, rows: &mut Vec<InspectorRow>, idx: &mut usize) {
        rows.push(InspectorRow::Header {
            name: sv.name.clone(),
            depth,
        });
        for fv in &sv.fields {
            rows.push(InspectorRow::Field {
                field_index: *idx,
                name: fv.def.name.clone(),
                display: fv.display.clone(),
                abs_offset: fv.abs_offset,
                size: fv.size,
                depth: depth + 1,
                editable: fv.def.editable,
            });
            *idx += 1;
        }
        for child in &sv.children {
            walk(child, depth + 1, rows, idx);
        }
    }
    let mut rows = Vec::new();
    let mut idx = 0;
    for sv in structs {
        walk(sv, 0, &mut rows, &mut idx);
    }
    rows
}

/// Read bytes from the document at the given offset and length.
fn read_bytes(doc: &mut Document, offset: u64, len: usize) -> HxResult<Vec<u8>> {
    let mut buf = Vec::with_capacity(len);
    for i in 0..len {
        match doc.byte_at(offset + i as u64)? {
            ByteSlot::Present(b) => buf.push(b),
            _ => buf.push(0),
        }
    }
    Ok(buf)
}

/// Parse a single field: read raw bytes from the document and format as display string.
fn parse_field(doc: &mut Document, field: &FieldDef, base_offset: u64) -> HxResult<FieldValue> {
    let abs_offset = base_offset + field.offset;
    let (raw_bytes, size, display) = match &field.field_type {
        FieldType::DataRange(len) => {
            let len = *len;
            let end = abs_offset + len;
            let display = if len == 0 {
                "empty".to_owned()
            } else {
                format!("0x{:x}–0x{:x} ({} bytes)", abs_offset, end - 1, len)
            };
            (Vec::new(), len as usize, display)
        }
        _ => {
            let size = field.field_type.byte_size().unwrap_or(0);
            let raw_bytes = read_bytes(doc, abs_offset, size)?;
            let display = format_value(&field.field_type, &raw_bytes);
            (raw_bytes, size, display)
        }
    };
    Ok(FieldValue {
        def: field.clone(),
        abs_offset,
        raw_bytes,
        display,
        size,
    })
}

/// Parse a complete format definition, producing a StructValue tree.
pub fn parse_format(def: &FormatDef, doc: &mut Document) -> HxResult<Vec<StructValue>> {
    def.structs.iter().map(|sd| parse_struct(doc, sd)).collect()
}

fn parse_struct(doc: &mut Document, sd: &StructDef) -> HxResult<StructValue> {
    let fields: Vec<FieldValue> = sd
        .fields
        .iter()
        .map(|fd| parse_field(doc, fd, sd.base_offset))
        .collect::<HxResult<_>>()?;
    let children: Vec<StructValue> = sd
        .children
        .iter()
        .map(|child| parse_struct(doc, child))
        .collect::<HxResult<_>>()?;
    Ok(StructValue {
        name: sd.name.clone(),
        base_offset: sd.base_offset,
        fields,
        children,
    })
}

/// Decode raw bytes as an unsigned u64 value based on the field type.
pub fn decode_unsigned(field_type: &FieldType, raw: &[u8]) -> u64 {
    match field_type {
        FieldType::U8 => raw.first().copied().unwrap_or(0) as u64,
        FieldType::U16Le => u16::from_le_bytes(
            raw.get(..2)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 2]),
        ) as u64,
        FieldType::U16Be => u16::from_be_bytes(
            raw.get(..2)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 2]),
        ) as u64,
        FieldType::U32Le => u32::from_le_bytes(
            raw.get(..4)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 4]),
        ) as u64,
        FieldType::U32Be => u32::from_be_bytes(
            raw.get(..4)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 4]),
        ) as u64,
        FieldType::U64Le => u64::from_le_bytes(
            raw.get(..8)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 8]),
        ),
        FieldType::U64Be => u64::from_be_bytes(
            raw.get(..8)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 8]),
        ),
        _ => 0,
    }
}

/// Format raw bytes according to FieldType into a human-readable string.
pub fn format_value(field_type: &FieldType, raw: &[u8]) -> String {
    match field_type {
        FieldType::U8 => format!("0x{:02x}", raw.first().copied().unwrap_or(0)),
        FieldType::U16Le => {
            let v = u16::from_le_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("0x{:04x}", v)
        }
        FieldType::U16Be => {
            let v = u16::from_be_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("0x{:04x}", v)
        }
        FieldType::U32Le => {
            let v = u32::from_le_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("0x{:08x}", v)
        }
        FieldType::U32Be => {
            let v = u32::from_be_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("0x{:08x}", v)
        }
        FieldType::U64Le => {
            let v = u64::from_le_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("0x{:016x}", v)
        }
        FieldType::U64Be => {
            let v = u64::from_be_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("0x{:016x}", v)
        }
        FieldType::I8 => format!("{}", raw.first().copied().unwrap_or(0) as i8),
        FieldType::I16Le => {
            let v = i16::from_le_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("{}", v)
        }
        FieldType::I16Be => {
            let v = i16::from_be_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("{}", v)
        }
        FieldType::I32Le => {
            let v = i32::from_le_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("{}", v)
        }
        FieldType::I32Be => {
            let v = i32::from_be_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("{}", v)
        }
        FieldType::I64Le => {
            let v = i64::from_le_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("{}", v)
        }
        FieldType::I64Be => {
            let v = i64::from_be_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("{}", v)
        }
        FieldType::Bytes(n) => raw
            .iter()
            .take(*n)
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" "),
        FieldType::Utf8(n) => {
            let s = String::from_utf8_lossy(&raw[..*n.min(&raw.len())]);
            format!("\"{}\"", s.trim_end_matches('\0'))
        }
        FieldType::DataRange(len) => {
            if *len == 0 {
                "empty".to_owned()
            } else {
                format!("{} bytes", len)
            }
        }
        FieldType::Enum { inner, variants } => {
            let base = format_value(inner, raw);
            let numeric = decode_unsigned(inner, raw);
            let label = variants
                .iter()
                .find(|(v, _)| *v == numeric)
                .map(|(_, name)| name.as_str())
                .unwrap_or("?");
            format!("{} ({})", base, label)
        }
        FieldType::Flags { inner, flags } => {
            let base = format_value(inner, raw);
            let numeric = decode_unsigned(inner, raw);
            let active: Vec<&str> = flags
                .iter()
                .filter(|(bit, _)| numeric & bit != 0)
                .map(|(_, name)| name.as_str())
                .collect();
            if active.is_empty() {
                base
            } else {
                format!("{} [{}]", base, active.join(" | "))
            }
        }
    }
}
