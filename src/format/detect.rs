use crate::core::document::Document;
use crate::format::defs;
use crate::format::types::FormatDef;

/// Default per-format entry cap used when the UI layer has not requested a
/// higher value. 64 keeps ELF repeated tables / PNG chunk count / ZIP local
/// file header lists at a manageable inspector height on first open;
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
/// that support pagination (ELF / PNG / ZIP).
pub fn detect_format_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if let Some(def) = defs::elf::detect_with_cap(doc, entry_cap) {
        return Some(def);
    }
    if let Some(def) = defs::png::detect_with_cap(doc, entry_cap) {
        return Some(def);
    }
    if let Some(def) = defs::zip::detect_with_cap(doc, entry_cap) {
        return Some(def);
    }
    if let Some(def) = defs::gzip::detect_with_cap(doc, entry_cap) {
        return Some(def);
    }
    if let Some(def) = defs::tar::detect_with_cap(doc, entry_cap) {
        return Some(def);
    }
    None
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
    match name.to_lowercase().as_str() {
        "elf" => defs::elf::detect_with_cap(doc, entry_cap),
        "png" => defs::png::detect_with_cap(doc, entry_cap),
        "zip" => defs::zip::detect_with_cap(doc, entry_cap),
        "gzip" | "gz" => defs::gzip::detect_with_cap(doc, entry_cap),
        "tar" => defs::tar::detect_with_cap(doc, entry_cap),
        _ => None,
    }
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
