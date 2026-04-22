use crate::executable::types::{CodeSpan, Endian};

pub(super) fn demangle_symbol(name: &str) -> String {
    rustc_demangle::try_demangle(name)
        .map(|display| display.to_string())
        .unwrap_or_else(|_| name.to_owned())
}

pub(super) fn read_u16(buf: &[u8], off: usize, endian: Endian) -> Option<u16> {
    let s = buf.get(off..off + 2)?;
    Some(match endian {
        Endian::Little => u16::from_le_bytes([s[0], s[1]]),
        Endian::Big => u16::from_be_bytes([s[0], s[1]]),
    })
}

pub(super) fn read_u32(buf: &[u8], off: usize, endian: Endian) -> Option<u32> {
    let s = buf.get(off..off + 4)?;
    Some(match endian {
        Endian::Little => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        Endian::Big => u32::from_be_bytes([s[0], s[1], s[2], s[3]]),
    })
}

pub(super) fn read_u64(buf: &[u8], off: usize, endian: Endian) -> Option<u64> {
    let s = buf.get(off..off + 8)?;
    Some(match endian {
        Endian::Little => u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        Endian::Big => u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
    })
}

pub(super) fn push_span(spans: &mut Vec<CodeSpan>, span: CodeSpan) {
    if span.end_inclusive < span.start {
        return;
    }
    if spans.iter().any(|existing| {
        existing.start == span.start
            && existing.end_inclusive == span.end_inclusive
            && existing.virtual_start == span.virtual_start
            && existing.virtual_end_inclusive == span.virtual_end_inclusive
            && existing.name == span.name
    }) {
        return;
    }
    spans.push(span);
    spans.sort_by_key(|entry| (entry.start, entry.end_inclusive));
}
