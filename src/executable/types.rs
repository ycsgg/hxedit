#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableKind {
    Elf,
    Pe,
    MachO,
}

impl ExecutableKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Elf => "ELF",
            Self::Pe => "PE",
            Self::MachO => "Mach-O",
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
    pub name: Option<String>,
    pub executable: bool,
}

impl CodeSpan {
    pub fn contains(&self, offset: u64) -> bool {
        offset >= self.start && offset <= self.end_inclusive
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableInfo {
    pub kind: ExecutableKind,
    pub arch: ExecutableArch,
    pub bitness: Bitness,
    pub endian: Endian,
    pub entry_offset: Option<u64>,
    pub code_spans: Vec<CodeSpan>,
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
}
