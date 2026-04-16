use crate::app::{EditOp, ReplacementChange};
use crate::core::document::Document;
use crate::error::HxResult;
use crate::format::types::FieldType;

/// Encode a user-input text value back into raw bytes for writing to the document.
///
/// Supported input formats:
/// - Hex number: 0x1234 or 1234h
/// - Decimal number: 4660
/// - Enum name: ET_EXEC (for Enum types, matches variant name)
/// - Raw hex bytes: 7f 45 4c 46 (for Bytes type)
/// - String literal: "text" (for Utf8 type)
pub fn encode_value(field_type: &FieldType, input: &str) -> Result<Vec<u8>, String> {
    let input = input.trim();
    match field_type {
        FieldType::U8 => {
            let v = parse_u64(input)?;
            encode_unsigned(v, u8::MAX as u64, |value| vec![value as u8])
        }
        FieldType::U16Le => {
            let v = parse_u64(input)?;
            encode_unsigned(v, u16::MAX as u64, |value| {
                (value as u16).to_le_bytes().to_vec()
            })
        }
        FieldType::U16Be => {
            let v = parse_u64(input)?;
            encode_unsigned(v, u16::MAX as u64, |value| {
                (value as u16).to_be_bytes().to_vec()
            })
        }
        FieldType::U32Le => {
            let v = parse_u64(input)?;
            encode_unsigned(v, u32::MAX as u64, |value| {
                (value as u32).to_le_bytes().to_vec()
            })
        }
        FieldType::U32Be => {
            let v = parse_u64(input)?;
            encode_unsigned(v, u32::MAX as u64, |value| {
                (value as u32).to_be_bytes().to_vec()
            })
        }
        FieldType::U64Le => {
            let v = parse_u64(input)?;
            Ok(v.to_le_bytes().to_vec())
        }
        FieldType::U64Be => {
            let v = parse_u64(input)?;
            Ok(v.to_be_bytes().to_vec())
        }
        FieldType::I8 => {
            let v = parse_i64(input)?;
            encode_signed(v, i8::MIN as i64, i8::MAX as i64, |value| {
                vec![value as i8 as u8]
            })
        }
        FieldType::I16Le => {
            let v = parse_i64(input)?;
            encode_signed(v, i16::MIN as i64, i16::MAX as i64, |value| {
                (value as i16).to_le_bytes().to_vec()
            })
        }
        FieldType::I16Be => {
            let v = parse_i64(input)?;
            encode_signed(v, i16::MIN as i64, i16::MAX as i64, |value| {
                (value as i16).to_be_bytes().to_vec()
            })
        }
        FieldType::I32Le => {
            let v = parse_i64(input)?;
            encode_signed(v, i32::MIN as i64, i32::MAX as i64, |value| {
                (value as i32).to_le_bytes().to_vec()
            })
        }
        FieldType::I32Be => {
            let v = parse_i64(input)?;
            encode_signed(v, i32::MIN as i64, i32::MAX as i64, |value| {
                (value as i32).to_be_bytes().to_vec()
            })
        }
        FieldType::I64Le => {
            let v = parse_i64(input)?;
            Ok(v.to_le_bytes().to_vec())
        }
        FieldType::I64Be => {
            let v = parse_i64(input)?;
            Ok(v.to_be_bytes().to_vec())
        }
        FieldType::Bytes(n) => {
            let bytes: Result<Vec<u8>, String> = input
                .split_whitespace()
                .map(|tok| {
                    u8::from_str_radix(tok, 16).map_err(|_| format!("invalid hex byte: {}", tok))
                })
                .collect();
            let bytes = bytes?;
            if bytes.len() != *n {
                return Err(format!("expected {} bytes, got {}", n, bytes.len()));
            }
            Ok(bytes)
        }
        FieldType::Utf8(n) => {
            let s = input.trim_matches('"');
            if s.len() > *n {
                return Err(format!("string too long: expected at most {} bytes", n));
            }
            let mut bytes = s.as_bytes().to_vec();
            bytes.resize(*n, 0);
            Ok(bytes)
        }
        FieldType::DataRange(_) => Err("data range fields cannot be edited".into()),
        FieldType::Enum { inner, variants } => {
            // Try matching variant name first
            if let Some((val, _)) = variants.iter().find(|(_, name)| name == input) {
                return encode_value(inner, &format!("0x{:x}", val));
            }
            // Fall back to numeric input
            encode_value(inner, input)
        }
        FieldType::Flags { inner, .. } => {
            // For flags, accept numeric input only
            encode_value(inner, input)
        }
    }
}

/// Write encoded bytes to the document at the given absolute offset.
pub(crate) fn write_field(
    doc: &mut Document,
    abs_offset: u64,
    bytes: &[u8],
) -> HxResult<Vec<EditOp>> {
    let mut changes = Vec::new();
    for (i, &byte) in bytes.iter().enumerate() {
        let offset = abs_offset + i as u64;
        if offset < doc.len() {
            let Some(id) = doc.cell_id_at(offset) else {
                continue;
            };
            let previous = doc.replacement_state(id);
            doc.replace_display_byte(offset, byte)?;
            let after = doc.replacement_state(id);
            if after != previous {
                changes.push(ReplacementChange {
                    id,
                    before: previous,
                    after,
                });
            }
        }
    }
    if changes.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![EditOp::ReplaceBytes { changes }])
    }
}

fn parse_u64(input: &str) -> Result<u64, String> {
    let input = numeric_token(input);
    if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex: {}", e))
    } else if let Some(hex) = input.strip_suffix('h').or_else(|| input.strip_suffix('H')) {
        u64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex: {}", e))
    } else {
        input
            .parse::<u64>()
            .map_err(|e| format!("invalid number: {}", e))
    }
}

fn parse_i64(input: &str) -> Result<i64, String> {
    let input = numeric_token(input);
    if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex: {}", e))
    } else {
        input
            .parse::<i64>()
            .map_err(|e| format!("invalid number: {}", e))
    }
}

fn numeric_token(input: &str) -> &str {
    let trimmed = input.trim();
    trimmed
        .split([' ', '\t', '(', '[', '{'])
        .next()
        .unwrap_or(trimmed)
}

fn encode_unsigned<F>(value: u64, max: u64, encode: F) -> Result<Vec<u8>, String>
where
    F: FnOnce(u64) -> Vec<u8>,
{
    if value > max {
        return Err(format!("value out of range: {}", value));
    }
    Ok(encode(value))
}

fn encode_signed<F>(value: i64, min: i64, max: i64, encode: F) -> Result<Vec<u8>, String>
where
    F: FnOnce(i64) -> Vec<u8>,
{
    if value < min || value > max {
        return Err(format!("value out of range: {}", value));
    }
    Ok(encode(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_display_value_can_roundtrip_from_prefilled_text() {
        let encoded = encode_value(
            &FieldType::Enum {
                inner: Box::new(FieldType::U16Le),
                variants: vec![(0x3e, "EM_X86_64".into())],
            },
            "0x003e (EM_X86_64)",
        )
        .unwrap();
        assert_eq!(encoded, 0x003eu16.to_le_bytes());
    }

    #[test]
    fn flags_display_value_accepts_numeric_prefix() {
        let encoded = encode_value(
            &FieldType::Flags {
                inner: Box::new(FieldType::U16Le),
                flags: vec![(0x0001, "Encrypted".into())],
            },
            "0x0001 [Encrypted]",
        )
        .unwrap();
        assert_eq!(encoded, 1u16.to_le_bytes());
    }

    #[test]
    fn unsigned_values_are_range_checked() {
        assert!(encode_value(&FieldType::U8, "0x1ff").is_err());
    }
}
