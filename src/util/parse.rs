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
