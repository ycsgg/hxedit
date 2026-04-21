use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

const TAR_BLOCK: u64 = 512;
const TAR_NAME_LEN: usize = 100;
const TAR_MODE_LEN: usize = 8;
const TAR_UID_LEN: usize = 8;
const TAR_GID_LEN: usize = 8;
const TAR_SIZE_LEN: usize = 12;
const TAR_MTIME_LEN: usize = 12;
const TAR_CHECKSUM_LEN: usize = 8;
const TAR_LINKNAME_LEN: usize = 100;
const TAR_MAGIC_LEN: usize = 6;
const TAR_VERSION_LEN: usize = 2;
const TAR_UNAME_LEN: usize = 32;
const TAR_GNAME_LEN: usize = 32;
const TAR_DEV_LEN: usize = 8;
const TAR_PREFIX_LEN: usize = 155;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < TAR_BLOCK {
        return None;
    }

    let first = read_header(doc, 0)?;
    if !is_valid_tar_header(&first) {
        return None;
    }

    let mut structs = Vec::new();
    let mut offset = 0_u64;
    let mut entry_idx = 0_usize;
    let mut more_remain = false;

    while offset.saturating_add(TAR_BLOCK) <= doc.len() {
        if entry_idx >= entry_cap.max(1) {
            if let Some(next) = read_header(doc, offset) {
                if !is_zero_block(&next) && is_valid_tar_header(&next) {
                    more_remain = true;
                }
            }
            break;
        }

        let Some(header) = read_header(doc, offset) else {
            break;
        };
        if is_zero_block(&header) {
            break;
        }
        if !is_valid_tar_header(&header) {
            break;
        }

        let size = parse_octal_field(&header[124..136]).unwrap_or(0);
        let typeflag = header[156];
        let name = entry_name(&header);
        let header_end = offset.saturating_add(TAR_BLOCK);
        let data_end = header_end.saturating_add(size);
        let truncated = data_end > doc.len();

        let mut fields = vec![
            FieldDef {
                name: "name".into(),
                offset: 0,
                field_type: FieldType::Utf8(TAR_NAME_LEN),
                description: "Entry path".into(),
                editable: true,
            },
            FieldDef {
                name: "mode".into(),
                offset: 100,
                field_type: FieldType::Utf8(TAR_MODE_LEN),
                description: "File mode (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "uid".into(),
                offset: 108,
                field_type: FieldType::Utf8(TAR_UID_LEN),
                description: "Owner user ID (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "gid".into(),
                offset: 116,
                field_type: FieldType::Utf8(TAR_GID_LEN),
                description: "Owner group ID (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "size".into(),
                offset: 124,
                field_type: FieldType::Utf8(TAR_SIZE_LEN),
                description: "Entry size (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "mtime".into(),
                offset: 136,
                field_type: FieldType::Utf8(TAR_MTIME_LEN),
                description: "Modification time (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "checksum".into(),
                offset: 148,
                field_type: FieldType::Utf8(TAR_CHECKSUM_LEN),
                description: "Header checksum (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "typeflag".into(),
                offset: 156,
                field_type: FieldType::Enum {
                    inner: Box::new(FieldType::U8),
                    variants: typeflag_variants(),
                },
                description: "Entry type".into(),
                editable: true,
            },
            FieldDef {
                name: "linkname".into(),
                offset: 157,
                field_type: FieldType::Utf8(TAR_LINKNAME_LEN),
                description: "Target name for link entries".into(),
                editable: true,
            },
            FieldDef {
                name: "magic".into(),
                offset: 257,
                field_type: FieldType::Utf8(TAR_MAGIC_LEN),
                description: "USTAR magic".into(),
                editable: true,
            },
            FieldDef {
                name: "version".into(),
                offset: 263,
                field_type: FieldType::Utf8(TAR_VERSION_LEN),
                description: "USTAR version".into(),
                editable: true,
            },
            FieldDef {
                name: "uname".into(),
                offset: 265,
                field_type: FieldType::Utf8(TAR_UNAME_LEN),
                description: "Owner user name".into(),
                editable: true,
            },
            FieldDef {
                name: "gname".into(),
                offset: 297,
                field_type: FieldType::Utf8(TAR_GNAME_LEN),
                description: "Owner group name".into(),
                editable: true,
            },
            FieldDef {
                name: "devmajor".into(),
                offset: 329,
                field_type: FieldType::Utf8(TAR_DEV_LEN),
                description: "Major device number (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "devminor".into(),
                offset: 337,
                field_type: FieldType::Utf8(TAR_DEV_LEN),
                description: "Minor device number (octal text)".into(),
                editable: true,
            },
            FieldDef {
                name: "prefix".into(),
                offset: 345,
                field_type: FieldType::Utf8(TAR_PREFIX_LEN),
                description: "Path prefix".into(),
                editable: true,
            },
        ];

        if !truncated && size > 0 && typeflag_has_data(typeflag) {
            fields.push(FieldDef {
                name: "file_data".into(),
                offset: TAR_BLOCK,
                field_type: FieldType::DataRange(size),
                description: "Entry payload bytes".into(),
                editable: false,
            });
        }

        structs.push(StructDef {
            name: if truncated {
                format!(
                    "Entry {}: {} [{}] (truncated)",
                    entry_idx,
                    display_name(&name),
                    typeflag_label(typeflag)
                )
            } else {
                format!(
                    "Entry {}: {} [{}]",
                    entry_idx,
                    display_name(&name),
                    typeflag_label(typeflag)
                )
            },
            base_offset: offset,
            fields,
            children: vec![],
        });

        entry_idx += 1;
        if truncated {
            break;
        }

        offset = header_end.saturating_add(pad_to_block(size));
    }

    if structs.is_empty() {
        return None;
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "… more entries beyond {} (use `:insp more` to load more)",
                entry_idx
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "TAR".to_string(),
        structs,
    })
}

fn read_header(doc: &mut Document, offset: u64) -> Option<Vec<u8>> {
    read_bytes_raw(doc, offset, TAR_BLOCK as usize)
}

fn is_valid_tar_header(header: &[u8]) -> bool {
    !is_zero_block(header) && has_ustar_magic(header) && checksum_matches(header)
}

fn has_ustar_magic(header: &[u8]) -> bool {
    header.len() >= TAR_BLOCK as usize
        && header.get(257..262).is_some_and(|magic| magic == b"ustar")
}

fn checksum_matches(header: &[u8]) -> bool {
    let Some(stored) = parse_octal_field(header.get(148..156).unwrap_or(&[])) else {
        return false;
    };
    compute_checksum(header) == stored
}

fn compute_checksum(header: &[u8]) -> u64 {
    header
        .iter()
        .enumerate()
        .map(|(idx, byte)| {
            if (148..156).contains(&idx) {
                b' ' as u64
            } else {
                *byte as u64
            }
        })
        .sum()
}

fn parse_octal_field(raw: &[u8]) -> Option<u64> {
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
    let text = std::str::from_utf8(&raw[..end]).ok()?.trim();
    if text.is_empty() {
        Some(0)
    } else {
        u64::from_str_radix(text, 8).ok()
    }
}

fn entry_name(header: &[u8]) -> String {
    let name = trim_tar_text(&header[..TAR_NAME_LEN]);
    let prefix = trim_tar_text(&header[345..500]);
    if prefix.is_empty() {
        name
    } else if name.is_empty() {
        prefix
    } else {
        format!("{prefix}/{name}")
    }
}

fn trim_tar_text(raw: &[u8]) -> String {
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end])
        .trim_end_matches(' ')
        .to_string()
}

fn display_name(name: &str) -> &str {
    if name.is_empty() {
        "<unnamed>"
    } else {
        name
    }
}

fn typeflag_has_data(typeflag: u8) -> bool {
    !matches!(typeflag, b'1' | b'2' | b'3' | b'4' | b'5' | b'6')
}

fn typeflag_variants() -> Vec<(u64, String)> {
    vec![
        (0, "Regular file".into()),
        (b'0' as u64, "Regular file".into()),
        (b'1' as u64, "Hard link".into()),
        (b'2' as u64, "Symlink".into()),
        (b'3' as u64, "Character device".into()),
        (b'4' as u64, "Block device".into()),
        (b'5' as u64, "Directory".into()),
        (b'6' as u64, "FIFO".into()),
        (b'7' as u64, "Contiguous file".into()),
        (b'g' as u64, "PAX global header".into()),
        (b'x' as u64, "PAX extended header".into()),
        (b'L' as u64, "GNU long name".into()),
        (b'K' as u64, "GNU long link".into()),
    ]
}

fn typeflag_label(typeflag: u8) -> &'static str {
    match typeflag {
        0 | b'0' => "file",
        b'1' => "hardlink",
        b'2' => "symlink",
        b'3' => "chardev",
        b'4' => "blockdev",
        b'5' => "dir",
        b'6' => "fifo",
        b'7' => "contig",
        b'g' => "pax-global",
        b'x' => "pax",
        b'L' => "gnu-longname",
        b'K' => "gnu-longlink",
        _ => "other",
    }
}

fn pad_to_block(size: u64) -> u64 {
    let rem = size % TAR_BLOCK;
    if rem == 0 {
        size
    } else {
        size + (TAR_BLOCK - rem)
    }
}

fn is_zero_block(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte == 0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap, TAR_BLOCK};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format;

    struct TarEntry<'a> {
        name: &'a str,
        typeflag: u8,
        data: &'a [u8],
        prefix: &'a str,
    }

    fn write_tar(path: &std::path::Path, bytes: &[u8]) -> Document {
        fs::write(path, bytes).unwrap();
        Document::open(path, &Config::default()).unwrap()
    }

    fn build_tar(entries: &[TarEntry<'_>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for entry in entries {
            let mut header = [0_u8; TAR_BLOCK as usize];
            write_bytes(&mut header[0..100], entry.name.as_bytes());
            write_octal(&mut header[100..108], 0o644);
            write_octal(&mut header[108..116], 0);
            write_octal(&mut header[116..124], 0);
            write_octal(&mut header[124..136], entry.data.len() as u64);
            write_octal(&mut header[136..148], 0);
            header[148..156].fill(b' ');
            header[156] = entry.typeflag;
            header[257..263].copy_from_slice(b"ustar\0");
            header[263..265].copy_from_slice(b"00");
            write_bytes(&mut header[265..297], b"root");
            write_bytes(&mut header[297..329], b"root");
            write_bytes(&mut header[345..500], entry.prefix.as_bytes());
            let checksum: u64 = header.iter().map(|&byte| byte as u64).sum();
            write_checksum(&mut header[148..156], checksum);
            bytes.extend_from_slice(&header);
            bytes.extend_from_slice(entry.data);
            let padded = super::pad_to_block(entry.data.len() as u64) as usize;
            bytes.resize(bytes.len() + (padded - entry.data.len()), 0);
        }
        bytes.resize(bytes.len() + (TAR_BLOCK as usize * 2), 0);
        bytes
    }

    fn write_bytes(dst: &mut [u8], src: &[u8]) {
        let len = src.len().min(dst.len());
        dst[..len].copy_from_slice(&src[..len]);
    }

    fn write_octal(dst: &mut [u8], value: u64) {
        dst.fill(0);
        let digits = dst.len().saturating_sub(1);
        let text = format!("{value:0digits$o}");
        dst[..digits].copy_from_slice(text.as_bytes());
        dst[digits] = 0;
    }

    fn write_checksum(dst: &mut [u8], checksum: u64) {
        let text = format!("{checksum:06o}\0 ");
        dst.copy_from_slice(text.as_bytes());
    }

    #[test]
    fn detects_entries_and_pagination_marker() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.tar");
        let entries = [
            TarEntry {
                name: "bin/hello",
                typeflag: b'0',
                data: b"abc",
                prefix: "",
            },
            TarEntry {
                name: "notes.txt",
                typeflag: b'0',
                data: b"xyz",
                prefix: "",
            },
            TarEntry {
                name: "docs",
                typeflag: b'5',
                data: b"",
                prefix: "",
            },
        ];
        let mut doc = write_tar(&path, &build_tar(&entries));

        let def = detect_with_cap(&mut doc, 2).expect("tar detected");
        assert_eq!(def.name, "TAR");
        assert!(def.structs[0].name.contains("bin/hello"));
        assert!(def.structs[1].name.contains("notes.txt"));
        assert!(def
            .structs
            .last()
            .is_some_and(|last| last.name.contains("more entries")));
    }

    #[test]
    fn file_data_range_points_to_payload_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("payload.tar");
        let entries = [TarEntry {
            name: "payload.bin",
            typeflag: b'0',
            data: b"hello",
            prefix: "",
        }];
        let mut doc = write_tar(&path, &build_tar(&entries));

        let def = detect(&mut doc).expect("tar detected");
        let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
        let entry = structs
            .iter()
            .find(|structure| structure.name.contains("payload.bin"))
            .expect("entry exists");
        let field = entry
            .fields
            .iter()
            .find(|field| field.def.name == "file_data")
            .expect("file_data field");
        assert_eq!(field.abs_offset, TAR_BLOCK);
        assert_eq!(field.size, 5);
    }

    #[test]
    fn rejects_invalid_checksum() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.tar");
        let entries = [TarEntry {
            name: "payload.bin",
            typeflag: b'0',
            data: b"hello",
            prefix: "",
        }];
        let mut bytes = build_tar(&entries);
        bytes[148..156].copy_from_slice(b"0000000\0");
        let mut doc = write_tar(&path, &bytes);
        assert!(detect(&mut doc).is_none());
    }
}
