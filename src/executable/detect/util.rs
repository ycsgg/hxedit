#[cfg(feature = "symbols")]
use symbolic_common::Name;
#[cfg(feature = "symbols")]
use symbolic_demangle::{Demangle, DemangleOptions};

use crate::executable::types::{CodeSpan, Endian};

#[cfg(feature = "symbols")]
pub(super) fn demangle_symbol(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return String::new();
    }

    // symbolic-demangle supports C++ (GCC/MSVC), Rust, Swift, etc.
    let symbol = Name::from(name);
    let demangled = symbol.try_demangle(DemangleOptions::name_only());

    // Check if demangling actually changed the name
    if demangled != name {
        return demangled.to_string();
    }

    // Try stripping leading underscore for Mach-O / C symbols
    if let Some(stripped) = name.strip_prefix('_') {
        let symbol = Name::from(stripped);
        let demangled = symbol.try_demangle(DemangleOptions::name_only());
        if demangled != stripped {
            return demangled.to_string();
        }
    }

    normalize_symbol_name(name).to_owned()
}

#[cfg_attr(not(feature = "symbols"), allow(dead_code))]
#[cfg(not(feature = "symbols"))]
pub(super) fn demangle_symbol(name: &str) -> String {
    normalize_symbol_name(name.trim()).to_owned()
}

#[cfg_attr(not(feature = "symbols"), allow(dead_code))]
fn normalize_symbol_name(name: &str) -> &str {
    let mut name = strip_import_prefix(name);
    name = strip_single_leading_underscore(name);
    name = strip_elf_suffix(name);
    name = strip_stdcall_suffix(name);
    name
}

#[cfg_attr(not(feature = "symbols"), allow(dead_code))]
fn strip_import_prefix(name: &str) -> &str {
    name.strip_prefix("__imp_").unwrap_or(name)
}

#[cfg_attr(not(feature = "symbols"), allow(dead_code))]
fn strip_single_leading_underscore(name: &str) -> &str {
    let Some(stripped) = name.strip_prefix('_') else {
        return name;
    };
    if stripped.starts_with('_') {
        return name;
    }
    let Some(first) = stripped.chars().next() else {
        return name;
    };
    if first.is_ascii_alphanumeric() {
        stripped
    } else {
        name
    }
}

#[cfg_attr(not(feature = "symbols"), allow(dead_code))]
fn strip_elf_suffix(name: &str) -> &str {
    if let Some(stripped) = name.strip_suffix("@plt") {
        return stripped;
    }
    if let Some((base, _)) = name.split_once("@@") {
        if !base.is_empty() {
            return base;
        }
    }
    if let Some((base, suffix)) = name.rsplit_once('@') {
        if !base.is_empty() && !suffix.is_empty() && !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return base;
        }
    }
    name
}

#[cfg_attr(not(feature = "symbols"), allow(dead_code))]
fn strip_stdcall_suffix(name: &str) -> &str {
    let Some((base, suffix)) = name.rsplit_once('@') else {
        return name;
    };
    if !base.is_empty() && !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
        base
    } else {
        name
    }
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

#[cfg(test)]
mod tests {
    use super::demangle_symbol;

    #[test]
    fn normalizes_common_platform_symbol_decorations() {
        assert_eq!(demangle_symbol("_main"), "main");
        assert_eq!(demangle_symbol("puts@@GLIBC_2.2.5"), "puts");
        assert_eq!(demangle_symbol("puts@plt"), "puts");
        assert_eq!(demangle_symbol("__imp__CreateFileW@28"), "CreateFileW");
        assert_eq!(demangle_symbol("_MessageBoxA@16"), "MessageBoxA");
    }

    #[cfg(feature = "symbols")]
    #[test]
    fn demangles_cpp_symbols() {
        // GCC-style C++ mangling
        assert_eq!(demangle_symbol("_ZN3foo3barEv"), "foo::bar");
        assert_eq!(
            demangle_symbol("_ZNKSt9exceptionD0Ev"),
            "std::exception::~exception"
        );
        // More complex C++ symbol
        assert!(demangle_symbol("_ZN3std9basic_iosIcSt11char_traitsIcEED2Ev").contains("std"));
    }

    #[cfg(feature = "symbols")]
    #[test]
    fn demangles_rust_symbols() {
        // Legacy Rust mangling
        assert!(
            demangle_symbol("_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E")
                .contains("read_to_end")
        );
    }
}
