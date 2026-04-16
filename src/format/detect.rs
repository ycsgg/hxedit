use crate::core::document::Document;
use crate::format::defs;
use crate::format::types::FormatDef;

/// Try to auto-detect the file format.
///
/// Tries registered Rust built-in formats in priority order.
/// Returns the first matching format definition, or None.
pub fn detect_format(doc: &mut Document) -> Option<FormatDef> {
    let detectors: Vec<fn(&mut Document) -> Option<FormatDef>> =
        vec![defs::elf::detect, defs::png::detect, defs::zip::detect];

    for detector in detectors {
        if let Some(def) = detector(doc) {
            return Some(def);
        }
    }
    None
}

/// Detect a format by name (for `:format <name>` command).
pub fn detect_by_name(name: &str, doc: &mut Document) -> Option<FormatDef> {
    match name.to_lowercase().as_str() {
        "elf" => defs::elf::detect(doc),
        "png" => defs::png::detect(doc),
        "zip" => defs::zip::detect(doc),
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
