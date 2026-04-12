use crate::error::{HxError, HxResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFormat {
    Binary,
    Byte,
    DoubleByte,
    QuadByte,
}

impl CopyFormat {
    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "bin" | "binary" => Some(Self::Binary),
            "b" | "byte" => Some(Self::Byte),
            "db" => Some(Self::DoubleByte),
            "qb" => Some(Self::QuadByte),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Binary => "bin",
            Self::Byte => "b",
            Self::DoubleByte => "db",
            Self::QuadByte => "qb",
        }
    }

    pub fn group_size(self) -> usize {
        match self {
            Self::Binary | Self::Byte => 1,
            Self::DoubleByte => 2,
            Self::QuadByte => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyDisplay {
    Raw,
    NumericBig,
    NumericLittle,
}

impl CopyDisplay {
    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "r" | "raw" => Some(Self::Raw),
            "nb" => Some(Self::NumericBig),
            "nl" => Some(Self::NumericLittle),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::NumericBig => "nb",
            Self::NumericLittle => "nl",
        }
    }
}

pub fn format_selection(
    bytes: &[u8],
    format: CopyFormat,
    display: CopyDisplay,
) -> HxResult<String> {
    match display {
        CopyDisplay::Raw => Ok(format_raw(bytes, format)),
        CopyDisplay::NumericBig => format_numeric(bytes, format, true),
        CopyDisplay::NumericLittle => format_numeric(bytes, format, false),
    }
}

fn format_raw(bytes: &[u8], format: CopyFormat) -> String {
    match format {
        CopyFormat::Binary => bytes
            .iter()
            .map(|byte| format!("{byte:08b}"))
            .collect::<Vec<_>>()
            .join(" "),
        CopyFormat::Byte => bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
        CopyFormat::DoubleByte => bytes.chunks(2).map(hex_group).collect::<Vec<_>>().join(" "),
        CopyFormat::QuadByte => bytes.chunks(4).map(hex_group).collect::<Vec<_>>().join(" "),
    }
}

fn format_numeric(bytes: &[u8], format: CopyFormat, big_endian: bool) -> HxResult<String> {
    let group_size = format.group_size();
    if group_size > 1 && bytes.len() % group_size != 0 {
        return Err(HxError::CopyAlignment(group_size));
    }

    let values = bytes
        .chunks(group_size)
        .map(|chunk| match format {
            CopyFormat::Binary | CopyFormat::Byte => chunk[0] as u32,
            CopyFormat::DoubleByte => {
                let pair = [chunk[0], chunk[1]];
                if big_endian {
                    u16::from_be_bytes(pair) as u32
                } else {
                    u16::from_le_bytes(pair) as u32
                }
            }
            CopyFormat::QuadByte => {
                let quad = [chunk[0], chunk[1], chunk[2], chunk[3]];
                if big_endian {
                    u32::from_be_bytes(quad)
                } else {
                    u32::from_le_bytes(quad)
                }
            }
        })
        .map(|value| value.to_string())
        .collect::<Vec<_>>();

    Ok(values.join(" "))
}

fn hex_group(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_binary_raw_output() {
        assert_eq!(
            format_selection(&[0x0f, 0xa0], CopyFormat::Binary, CopyDisplay::Raw).unwrap(),
            "00001111 10100000"
        );
    }

    #[test]
    fn formats_hex_grouped_output() {
        assert_eq!(
            format_selection(
                &[0x12, 0x34, 0x56],
                CopyFormat::DoubleByte,
                CopyDisplay::Raw
            )
            .unwrap(),
            "1234 56"
        );
    }

    #[test]
    fn formats_numeric_endianness() {
        assert_eq!(
            format_selection(
                &[0x01, 0x02, 0x03, 0x04],
                CopyFormat::DoubleByte,
                CopyDisplay::NumericBig
            )
            .unwrap(),
            "258 772"
        );
        assert_eq!(
            format_selection(
                &[0x01, 0x02, 0x03, 0x04],
                CopyFormat::DoubleByte,
                CopyDisplay::NumericLittle
            )
            .unwrap(),
            "513 1027"
        );
    }

    #[test]
    fn numeric_grouping_requires_full_units() {
        assert!(matches!(
            format_selection(
                &[0x01, 0x02, 0x03],
                CopyFormat::DoubleByte,
                CopyDisplay::NumericBig
            ),
            Err(HxError::CopyAlignment(2))
        ));
    }
}
