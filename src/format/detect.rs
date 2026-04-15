use crate::core::document::{ByteSlot, Document};
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
    match doc.byte_at(offset).ok()? {
        ByteSlot::Present(b) => Some(b),
        _ => None,
    }
}

/// Helper: read N bytes from the document.
pub(crate) fn read_bytes_raw(doc: &mut Document, offset: u64, len: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(len);
    for i in 0..len {
        buf.push(read_u8(doc, offset + i as u64)?);
    }
    Some(buf)
}
