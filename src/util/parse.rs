use anyhow::{anyhow, Result};

use crate::error::HxError;

pub fn parse_offset(input: &str) -> Result<u64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(HxError::InvalidOffset(input.to_owned()).into());
    }

    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16)
            .map_err(|_| HxError::InvalidOffset(input.to_owned()).into());
    }

    trimmed
        .parse::<u64>()
        .map_err(|_| anyhow!(HxError::InvalidOffset(input.to_owned())))
}

pub fn parse_hex_nibble(c: char) -> Option<u8> {
    c.to_digit(16).map(|value| value as u8)
}

pub fn parse_hex_bytes(input: &str) -> Result<Vec<u8>, HxError> {
    let compact: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err(HxError::EmptySearch);
    }
    if compact.len() % 2 != 0 {
        return Err(HxError::InvalidHexPattern(input.to_owned()));
    }

    let mut out = Vec::with_capacity(compact.len() / 2);
    let bytes = compact.as_bytes();
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| HxError::InvalidHexPattern(input.to_owned()))?;
        let lo = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| HxError::InvalidHexPattern(input.to_owned()))?;
        out.push(((hi << 4) | lo) as u8);
    }
    Ok(out)
}

pub fn parse_hex_stream(input: &str) -> Result<Vec<u8>, HxError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(HxError::InvalidHexPattern(input.to_owned()));
    }

    let has_separator = trimmed.chars().any(|c| c.is_ascii_whitespace() || c == ',');

    if has_separator {
        let mut out = Vec::new();
        for token in trimmed
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|token| !token.is_empty())
        {
            let normalized = token
                .strip_prefix("0x")
                .or_else(|| token.strip_prefix("0X"))
                .unwrap_or(token);
            out.extend(parse_hex_bytes(normalized)?);
        }
        Ok(out)
    } else {
        let normalized = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        parse_hex_bytes(normalized)
    }
}

pub fn parse_paste_text_bytes(input: &str) -> Result<Vec<u8>, HxError> {
    if let Ok(hex) = parse_hex_stream(input) {
        return Ok(hex);
    }
    decode_base64(input).map_err(|_| HxError::InvalidPasteData(input.trim().to_owned()))
}

pub fn decode_base64(input: &str) -> Result<Vec<u8>, HxError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(HxError::InvalidPasteData(input.to_owned()));
    }

    let body = if let Some(idx) = trimmed.find(";base64,") {
        &trimmed[idx + ";base64,".len()..]
    } else {
        trimmed
    };

    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut chunk = [0_u8; 4];
    let mut chunk_len = 0;

    for byte in body.bytes().filter(|b| !b.is_ascii_whitespace()) {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => 64,
            _ => return Err(HxError::InvalidPasteData(input.to_owned())),
        };
        chunk[chunk_len] = value;
        chunk_len += 1;
        if chunk_len == 4 {
            decode_base64_chunk(&chunk, &mut out)?;
            chunk_len = 0;
        }
    }

    match chunk_len {
        0 => {}
        2 => {
            let padded = [chunk[0], chunk[1], 64, 64];
            decode_base64_chunk(&padded, &mut out)?;
        }
        3 => {
            let padded = [chunk[0], chunk[1], chunk[2], 64];
            decode_base64_chunk(&padded, &mut out)?;
        }
        _ => return Err(HxError::InvalidPasteData(input.to_owned())),
    }

    Ok(out)
}

fn decode_base64_chunk(chunk: &[u8; 4], out: &mut Vec<u8>) -> Result<(), HxError> {
    if chunk[0] == 64 || chunk[1] == 64 {
        return Err(HxError::InvalidPasteData("invalid base64 chunk".to_owned()));
    }
    out.push((chunk[0] << 2) | (chunk[1] >> 4));
    if chunk[2] != 64 {
        out.push((chunk[1] << 4) | (chunk[2] >> 2));
    }
    if chunk[3] != 64 {
        out.push((chunk[2] << 6) | chunk[3]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_stream_accepts_compact_and_separated_forms() {
        assert_eq!(
            parse_hex_stream("deadbeef").unwrap(),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
        assert_eq!(
            parse_hex_stream("de ad,be ef").unwrap(),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
        assert_eq!(parse_hex_stream("0xde,0xad").unwrap(), vec![0xde, 0xad]);
    }

    #[test]
    fn parse_paste_text_accepts_base64() {
        assert_eq!(parse_paste_text_bytes("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(
            parse_paste_text_bytes("data:image/png;base64,SGVsbG8=").unwrap(),
            b"Hello"
        );
    }

    #[test]
    fn parse_paste_text_prefers_hex_when_possible() {
        assert_eq!(
            parse_paste_text_bytes("deadbeef").unwrap(),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
    }

    #[test]
    fn parse_paste_text_accepts_urlsafe_base64() {
        assert_eq!(parse_paste_text_bytes("SGVsbG8").unwrap(), b"Hello");
        assert_eq!(
            parse_paste_text_bytes("SGVsbG8=").unwrap(),
            parse_paste_text_bytes("SGVsbG8").unwrap()
        );
    }
}
