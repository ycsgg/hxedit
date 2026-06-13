use crate::core::document::Document;
use crate::format::defs;
use crate::format::types::FormatDef;

type FormatDetector = fn(&mut Document, usize) -> Option<FormatDef>;

pub struct BuiltinFormat {
    display_name: &'static str,
    primary_name: &'static str,
    aliases: &'static [&'static str],
    detector: FormatDetector,
}

impl BuiltinFormat {
    pub fn display_name(&self) -> &'static str {
        self.display_name
    }

    pub fn primary_name(&self) -> &'static str {
        self.primary_name
    }

    pub fn aliases(&self) -> &'static [&'static str] {
        self.aliases
    }

    fn detects_name(&self, name: &str) -> bool {
        self.aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(name))
    }
}

pub const BUILTIN_FORMATS: &[BuiltinFormat] = &[
    BuiltinFormat {
        display_name: "ELF",
        primary_name: "elf",
        aliases: &["elf"],
        detector: defs::elf::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "PE/COFF",
        primary_name: "pe",
        aliases: &["pe", "pe32", "pe32+"],
        detector: defs::pe::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "PNG",
        primary_name: "png",
        aliases: &["png"],
        detector: defs::png::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "ZIP",
        primary_name: "zip",
        aliases: &["zip"],
        detector: defs::zip::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "SQLite",
        primary_name: "sqlite",
        aliases: &["sqlite", "sqlite3", "db"],
        detector: defs::sqlite::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "PCAPNG",
        primary_name: "pcapng",
        aliases: &["pcapng", "ntar"],
        detector: defs::pcapng::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "PCAP",
        primary_name: "pcap",
        aliases: &["pcap", "cap"],
        detector: defs::pcap::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "GZIP",
        primary_name: "gzip",
        aliases: &["gzip", "gz"],
        detector: defs::gzip::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "GIF",
        primary_name: "gif",
        aliases: &["gif"],
        detector: defs::gif::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "BMP",
        primary_name: "bmp",
        aliases: &["bmp"],
        detector: defs::bmp::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "WAV",
        primary_name: "wav",
        aliases: &["wav", "wave"],
        detector: defs::wav::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "JPEG",
        primary_name: "jpeg",
        aliases: &["jpeg", "jpg"],
        detector: defs::jpeg::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "TAR",
        primary_name: "tar",
        aliases: &["tar"],
        detector: defs::tar::detect_with_cap,
    },
    BuiltinFormat {
        display_name: "Mach-O",
        primary_name: "macho",
        aliases: &["macho", "mach-o"],
        detector: defs::macho::detect_with_cap,
    },
];

/// Default per-format entry cap used when the UI layer has not requested a
/// higher value. 64 keeps ELF repeated tables / PNG chunk count / ZIP entries /
/// SQLite pages / PCAP packets / PCAPNG blocks at a manageable inspector height on first open;
/// `:insp more` raises it in batches.
pub const DEFAULT_ENTRY_CAP: usize = 64;

/// Try to auto-detect the file format.
///
/// Tries registered Rust built-in formats in priority order.
/// Returns the first matching format definition, or None.
pub fn detect_format(doc: &mut Document) -> Option<FormatDef> {
    detect_format_with_cap(doc, DEFAULT_ENTRY_CAP)
}

/// Like `detect_format`, but threads a per-format entry cap through to parsers
/// that support pagination (ELF / PNG / ZIP / SQLite / PCAP / PCAPNG / GIF / WAV).
pub fn detect_format_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    BUILTIN_FORMATS
        .iter()
        .find_map(|format| (format.detector)(doc, entry_cap))
}

/// Detect a format by name (for `:format <name>` command).
pub fn detect_by_name(name: &str, doc: &mut Document) -> Option<FormatDef> {
    detect_by_name_with_cap(name, doc, DEFAULT_ENTRY_CAP)
}

pub fn detect_by_name_with_cap(
    name: &str,
    doc: &mut Document,
    entry_cap: usize,
) -> Option<FormatDef> {
    BUILTIN_FORMATS
        .iter()
        .find(|format| format.detects_name(name))
        .and_then(|format| (format.detector)(doc, entry_cap))
}

pub fn supported_format_display_list() -> String {
    join_names(
        BUILTIN_FORMATS.iter().map(BuiltinFormat::display_name),
        " / ",
    )
}

pub fn forced_format_primary_names() -> String {
    join_names(BUILTIN_FORMATS.iter().map(BuiltinFormat::primary_name), "|")
}

pub fn forced_format_alias_list() -> String {
    let aliases = BUILTIN_FORMATS.iter().flat_map(|format| {
        format
            .aliases()
            .iter()
            .copied()
            .filter(|alias| *alias != format.primary_name())
    });
    join_names(aliases, ", ")
}

fn join_names<'a>(names: impl Iterator<Item = &'a str>, separator: &str) -> String {
    let mut out = String::new();
    for name in names {
        if !out.is_empty() {
            out.push_str(separator);
        }
        out.push_str(name);
    }
    out
}

/// Helper: read a single byte from the document, returning None on failure.
pub(crate) fn read_u8(doc: &mut Document, offset: u64) -> Option<u8> {
    let buf = doc.read_logical_range(offset, 1).ok()?;
    buf.first().copied()
}

/// Helper: read N bytes from the document via a batched piece walk.
pub(crate) fn read_bytes_raw(doc: &mut Document, offset: u64, len: usize) -> Option<Vec<u8>> {
    let buf = doc.read_logical_range(offset, len).ok()?;
    if buf.len() == len {
        Some(buf)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_format_display_list_comes_from_registry() {
        assert_eq!(
            supported_format_display_list(),
            "ELF / PE/COFF / PNG / ZIP / SQLite / PCAPNG / PCAP / GZIP / GIF / BMP / WAV / JPEG / TAR / Mach-O"
        );
    }

    #[test]
    fn forced_format_names_and_aliases_come_from_registry() {
        assert_eq!(
            forced_format_primary_names(),
            "elf|pe|png|zip|sqlite|pcapng|pcap|gzip|gif|bmp|wav|jpeg|tar|macho"
        );
        assert_eq!(
            forced_format_alias_list(),
            "pe32, pe32+, sqlite3, db, ntar, cap, gz, wave, jpg, mach-o"
        );
    }

    #[test]
    fn registry_name_matching_is_case_insensitive() {
        let sqlite = BUILTIN_FORMATS
            .iter()
            .find(|format| format.primary_name() == "sqlite")
            .expect("sqlite format");
        assert!(sqlite.detects_name("SQLite3"));

        let macho = BUILTIN_FORMATS
            .iter()
            .find(|format| format.primary_name() == "macho")
            .expect("macho format");
        assert!(macho.detects_name("Mach-O"));
    }
}
