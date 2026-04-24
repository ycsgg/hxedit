use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::DataState;
use crate::view::palette::Palette;

const ROWS: &[&str] = &[
    "binary",
    "octal",
    "uint8",
    "int8",
    "uint16",
    "int16",
    "uint24",
    "int24",
    "uint32",
    "int32",
    "uint64",
    "int64",
    "ULEB128",
    "SLEB128",
    "float16",
    "bfloat16",
    "float32",
    "float64",
    "GUID",
    "ASCII",
    "UTF-8",
    "UTF-16",
    "GB18030",
    "BIG5",
    "SHIFT-JIS",
];

pub(crate) fn line_count() -> usize {
    ROWS.len() + 1
}

pub(crate) fn build_lines(state: &DataState, width: u16, palette: &Palette) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut lines = Vec::with_capacity(line_count());
    lines.push(Line::from(vec![
        Span::styled("Data", palette.inspector_header),
        Span::raw(format!(" @ 0x{:x}", state.base_offset)),
    ]));

    for label in ROWS {
        let selected = Some(*label) == state.selected_label.as_deref();
        let value = value_for(label, state);
        lines.push(row_line(label, &value, selected, width, palette));
    }
    lines
}

pub(crate) fn label_at_visual_row(visual_row: usize) -> Option<&'static str> {
    visual_row
        .checked_sub(1)
        .and_then(|index| ROWS.get(index).copied())
}

pub(crate) fn byte_len_for_label(label: &str, state: &DataState) -> Option<usize> {
    let bytes = state.bytes.as_slice();
    match label {
        "binary" | "octal" | "uint8" | "int8" => bytes_len(bytes, 1),
        "uint16" | "int16" | "float16" | "bfloat16" => bytes_len(bytes, 2),
        "uint24" | "int24" => bytes_len(bytes, 3),
        "uint32" | "int32" | "float32" => bytes_len(bytes, 4),
        "uint64" | "int64" | "float64" => bytes_len(bytes, 8),
        "GUID" => bytes_len(bytes, 16),
        "ULEB128" => leb128_len(bytes),
        "SLEB128" => leb128_len(bytes),
        "ASCII" => ascii_char(bytes).map(|(_, len)| len),
        "UTF-8" => utf8_char(bytes).map(|(_, len)| len),
        "UTF-16" => utf16_char(bytes).map(|(_, len)| len),
        "GB18030" | "BIG5" | "SHIFT-JIS" => None,
        _ => None,
    }
}

fn row_line(
    label: &str,
    value: &str,
    selected: bool,
    width: usize,
    palette: &Palette,
) -> Line<'static> {
    let label_width = label_width(width);
    let style = value_style(value, selected, palette);
    Line::from(vec![
        Span::styled(
            pad(label, label_width),
            if selected {
                style
            } else {
                palette.inspector_field
            },
        ),
        Span::raw(" "),
        Span::styled(
            truncate(value, width.saturating_sub(label_width + 1)),
            style,
        ),
    ])
}

fn label_width(width: usize) -> usize {
    if width >= 28 {
        10
    } else {
        8.min(width.saturating_sub(1))
    }
}

fn value_style(value: &str, selected: bool, palette: &Palette) -> Style {
    if selected {
        palette.inspector_active
    } else if value == "-" {
        palette.command_hint
    } else {
        palette.inspector_value
    }
}

fn value_for(label: &str, state: &DataState) -> String {
    match label {
        "binary" => read_uint(&state.bytes, 1)
            .map(|v| format!("{v:08b}"))
            .unwrap_or_else(dash),
        "octal" => read_uint(&state.bytes, 1)
            .map(|v| format!("{v:o}"))
            .unwrap_or_else(dash),
        "uint8" => read_uint(&state.bytes, 1).map_or_else(dash, |v| v.to_string()),
        "int8" => state
            .bytes
            .first()
            .map_or_else(dash, |byte| (*byte as i8).to_string()),
        "uint16" => read_uint(&state.bytes, 2).map_or_else(dash, |v| v.to_string()),
        "int16" => read_int(&state.bytes, 2).map_or_else(dash, |v| v.to_string()),
        "uint24" => read_uint(&state.bytes, 3).map_or_else(dash, |v| v.to_string()),
        "int24" => read_int(&state.bytes, 3).map_or_else(dash, |v| v.to_string()),
        "uint32" => read_uint(&state.bytes, 4).map_or_else(dash, |v| v.to_string()),
        "int32" => read_int(&state.bytes, 4).map_or_else(dash, |v| v.to_string()),
        "uint64" => read_uint(&state.bytes, 8).map_or_else(dash, |v| v.to_string()),
        "int64" => read_int(&state.bytes, 8).map_or_else(dash, |v| v.to_string()),
        "ULEB128" => read_uleb128(&state.bytes).map_or_else(dash, |(v, _)| v.to_string()),
        "SLEB128" => read_sleb128(&state.bytes).map_or_else(dash, |(v, _)| v.to_string()),
        "float16" => read_uint(&state.bytes, 2).map_or_else(dash, f16_to_string),
        "bfloat16" => read_uint(&state.bytes, 2).map_or_else(dash, bf16_to_string),
        "float32" => read_uint(&state.bytes, 4)
            .map(|v| f32::from_bits(v as u32).to_string())
            .unwrap_or_else(dash),
        "float64" => read_uint(&state.bytes, 8)
            .map(|v| f64::from_bits(v).to_string())
            .unwrap_or_else(dash),
        "GUID" => guid_string(&state.bytes).unwrap_or_else(dash),
        "ASCII" => ascii_char(&state.bytes).map_or_else(dash, |(c, _)| c.to_string()),
        "UTF-8" => utf8_char(&state.bytes).map_or_else(dash, |(c, _)| c.to_string()),
        "UTF-16" => utf16_char(&state.bytes).map_or_else(dash, |(c, _)| c.to_string()),
        "GB18030" | "BIG5" | "SHIFT-JIS" => dash(),
        _ => dash(),
    }
}

fn bytes_len(bytes: &[u8], len: usize) -> Option<usize> {
    (bytes.len() >= len).then_some(len)
}

fn read_uint(bytes: &[u8], len: usize) -> Option<u64> {
    bytes_len(bytes, len)?;
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(len).enumerate() {
        value |= u64::from(byte) << (index * 8);
    }
    Some(value)
}

fn read_int(bytes: &[u8], len: usize) -> Option<i64> {
    let unsigned = read_uint(bytes, len)?;
    let shift = 64_usize.saturating_sub(len * 8);
    Some(((unsigned << shift) as i64) >> shift)
}

fn leb128_len(bytes: &[u8]) -> Option<usize> {
    bytes
        .iter()
        .position(|byte| byte & 0x80 == 0)
        .map(|index| index + 1)
}

fn read_uleb128(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut result = 0_u64;
    for (index, byte) in bytes.iter().copied().enumerate() {
        let shift = index * 7;
        if shift >= 64 {
            return None;
        }
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((result, index + 1));
        }
    }
    None
}

fn read_sleb128(bytes: &[u8]) -> Option<(i64, usize)> {
    let mut result = 0_i64;
    let mut shift = 0_u32;
    for (index, byte) in bytes.iter().copied().enumerate() {
        result |= i64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && byte & 0x40 != 0 {
                result |= (!0_i64) << shift;
            }
            return Some((result, index + 1));
        }
    }
    None
}

fn f16_to_string(bits: u64) -> String {
    f32_from_f16(bits as u16).to_string()
}

fn bf16_to_string(bits: u64) -> String {
    f32::from_bits((bits as u32) << 16).to_string()
}

fn f32_from_f16(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exp = (bits >> 10) & 0x1f;
    let frac = u32::from(bits & 0x03ff);
    let f32_bits = match exp {
        0 if frac == 0 => sign,
        0 => {
            let mut mant = frac;
            let mut exponent = -14_i32;
            while mant & 0x0400 == 0 {
                mant <<= 1;
                exponent -= 1;
            }
            mant &= 0x03ff;
            sign | (((exponent + 127) as u32) << 23) | (mant << 13)
        }
        0x1f => sign | 0x7f80_0000 | (frac << 13),
        _ => sign | ((u32::from(exp) + 112) << 23) | (frac << 13),
    };
    f32::from_bits(f32_bits)
}

fn guid_string(bytes: &[u8]) -> Option<String> {
    bytes_len(bytes, 16)?;
    let d1 = read_uint(&bytes[0..4], 4)? as u32;
    let d2 = read_uint(&bytes[4..6], 2)? as u16;
    let d3 = read_uint(&bytes[6..8], 2)? as u16;
    Some(format!(
        "{d1:08x}-{d2:04x}-{d3:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

fn ascii_char(bytes: &[u8]) -> Option<(char, usize)> {
    let byte = *bytes.first()?;
    (byte.is_ascii_graphic() || byte == b' ').then_some((byte as char, 1))
}

fn utf8_char(bytes: &[u8]) -> Option<(char, usize)> {
    let first = *bytes.first()?;
    let len = if first < 0x80 {
        1
    } else if first & 0b1110_0000 == 0b1100_0000 {
        2
    } else if first & 0b1111_0000 == 0b1110_0000 {
        3
    } else if first & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        return None;
    };
    if bytes.len() < len {
        return None;
    }
    let text = std::str::from_utf8(&bytes[..len]).ok()?;
    let ch = text.chars().next()?;
    Some((ch, len))
}

fn utf16_char(bytes: &[u8]) -> Option<(char, usize)> {
    if bytes.len() < 2 {
        return None;
    }
    let first = u16::from_le_bytes([bytes[0], bytes[1]]);
    if (0xd800..=0xdbff).contains(&first) {
        if bytes.len() < 4 {
            return None;
        }
        let second = u16::from_le_bytes([bytes[2], bytes[3]]);
        let ch = char::decode_utf16([first, second]).next()?.ok()?;
        Some((ch, 4))
    } else {
        let ch = char::decode_utf16([first]).next()?.ok()?;
        Some((ch, 2))
    }
}

fn pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

fn truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn dash() -> String {
    "-".to_owned()
}
