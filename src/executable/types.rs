use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableKind {
    Elf,
    Pe,
    MachO,
    Raw,
}

impl ExecutableKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Elf => "ELF",
            Self::Pe => "PE",
            Self::MachO => "Mach-O",
            Self::Raw => "Raw",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableArch {
    X86,
    X86_64,
    Arm,
    AArch64,
    RiscV64,
    Unknown,
}

impl ExecutableArch {
    pub fn label(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::X86_64 => "x86_64",
            Self::Arm => "arm",
            Self::AArch64 => "aarch64",
            Self::RiscV64 => "riscv64",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

impl Endian {
    pub fn label(self) -> &'static str {
        match self {
            Self::Little => "little",
            Self::Big => "big",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bitness {
    Bit32,
    Bit64,
}

impl Bitness {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bit32 => "32-bit",
            Self::Bit64 => "64-bit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSpan {
    pub start: u64,
    pub end_inclusive: u64,
    pub virtual_start: Option<u64>,
    pub virtual_end_inclusive: Option<u64>,
    pub name: Option<String>,
    pub executable: bool,
}

impl CodeSpan {
    pub fn contains(&self, offset: u64) -> bool {
        offset >= self.start && offset <= self.end_inclusive
    }

    pub fn contains_virtual(&self, address: u64) -> bool {
        let (Some(start), Some(end)) = (self.virtual_start, self.virtual_end_inclusive) else {
            return false;
        };
        address >= start && address <= end
    }

    pub fn virtual_address_for_offset(&self, offset: u64) -> Option<u64> {
        if !self.contains(offset) {
            return None;
        }
        Some(
            self.virtual_start?
                .saturating_add(offset.saturating_sub(self.start)),
        )
    }

    pub fn file_offset_for_virtual(&self, address: u64) -> Option<u64> {
        if !self.contains_virtual(address) {
            return None;
        }
        Some(
            self.start
                .saturating_add(address.saturating_sub(self.virtual_start?)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolSource {
    Object,
    Dynamic,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    Unknown,
    Function,
    Object,
    Section,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    pub display_name: String,
    pub raw_name: Option<String>,
    pub source: SymbolSource,
    pub size: u64,
    pub symbol_type: SymbolType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    pub library: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableInfo {
    pub kind: ExecutableKind,
    pub arch: ExecutableArch,
    pub bitness: Bitness,
    pub endian: Endian,
    pub entry_offset: Option<u64>,
    pub entry_virtual_address: Option<u64>,
    pub code_spans: Vec<CodeSpan>,
    pub symbols_by_va: BTreeMap<u64, SymbolInfo>,
    pub target_names_by_va: Box<BTreeMap<u64, String>>,
    pub symbols_by_name: HashMap<String, u64>,
    pub imports: Vec<ImportInfo>,
}

impl ExecutableInfo {
    pub fn first_executable_span(&self) -> Option<&CodeSpan> {
        self.code_spans.iter().find(|span| span.executable)
    }

    pub fn span_containing(&self, offset: u64) -> Option<&CodeSpan> {
        self.code_spans
            .iter()
            .filter(|span| span.contains(offset))
            .min_by_key(|span| span.end_inclusive - span.start)
    }

    pub fn span_containing_virtual(&self, address: u64) -> Option<&CodeSpan> {
        self.code_spans
            .iter()
            .filter(|span| span.contains_virtual(address))
            .min_by_key(|span| span.end_inclusive - span.start)
    }

    pub fn virtual_address_for_offset(&self, offset: u64) -> Option<u64> {
        self.span_containing(offset)
            .and_then(|span| span.virtual_address_for_offset(offset))
    }

    pub fn file_offset_for_virtual(&self, address: u64) -> Option<u64> {
        self.span_containing_virtual(address)
            .and_then(|span| span.file_offset_for_virtual(address))
    }

    pub fn symbol_at_virtual(&self, address: u64) -> Option<&SymbolInfo> {
        self.symbols_by_va.get(&address)
    }

    pub fn target_name_at_virtual(&self, address: u64) -> Option<&str> {
        self.target_names_by_va.get(&address).map(String::as_str)
    }

    pub fn display_name_at_virtual(&self, address: u64) -> Option<&str> {
        self.symbol_at_virtual(address)
            .map(|symbol| symbol.display_name.as_str())
            .or_else(|| self.target_name_at_virtual(address))
    }

    pub fn symbol_at_offset(&self, offset: u64) -> Option<&SymbolInfo> {
        self.virtual_address_for_offset(offset)
            .and_then(|address| self.symbol_at_virtual(address))
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols_by_va.len()
    }

    pub fn import_count(&self) -> usize {
        self.imports.len()
    }
}
