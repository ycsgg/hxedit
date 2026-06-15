use std::ops::Deref;

const MAX_ROW_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowBytes {
    len: u8,
    bytes: [u8; MAX_ROW_BYTES],
}

impl RowBytes {
    pub fn from_slice(bytes: &[u8]) -> Self {
        assert!(
            bytes.len() <= MAX_ROW_BYTES,
            "disassembly row stores at most {MAX_ROW_BYTES} bytes, got {}",
            bytes.len()
        );
        let mut out = [0_u8; MAX_ROW_BYTES];
        out[..bytes.len()].copy_from_slice(bytes);
        Self {
            len: bytes.len() as u8,
            bytes: out,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }
}

impl Deref for RowBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl From<&[u8]> for RowBytes {
    fn from(bytes: &[u8]) -> Self {
        Self::from_slice(bytes)
    }
}

impl From<Vec<u8>> for RowBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_slice(&bytes)
    }
}

impl<const N: usize> From<[u8; N]> for RowBytes {
    fn from(bytes: [u8; N]) -> Self {
        Self::from_slice(&bytes)
    }
}

impl PartialEq<Vec<u8>> for RowBytes {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<RowBytes> for Vec<u8> {
    fn eq(&self, other: &RowBytes) -> bool {
        self.as_slice() == other.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub bytes: RowBytes,
    pub text: String,
    pub direct_target: Option<DirectBranchTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectBranchKind {
    Call,
    Jump,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectBranchTarget {
    pub kind: DirectBranchKind,
    pub virtual_address: u64,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisasmRowKind {
    Instruction,
    Data,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisasmFunctionBoundary {
    Entry,
    Body,
    Exit,
    EntryExit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisasmFunctionScope {
    pub name: String,
    pub entry_va: u64,
    pub boundary: DisasmFunctionBoundary,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisasmRow {
    pub offset: u64,
    pub virtual_address: Option<u64>,
    pub bytes: RowBytes,
    pub assembly_text: String,
    pub text: String,
    pub symbolized_names: Vec<String>,
    pub symbol_label: Option<String>,
    pub direct_target: Option<DirectBranchTarget>,
    pub function_scope: Option<DisasmFunctionScope>,
    pub span_name: Option<String>,
    pub kind: DisasmRowKind,
}

impl DisasmRow {
    pub fn data(
        offset: u64,
        virtual_address: Option<u64>,
        bytes: Vec<u8>,
        symbol_label: Option<String>,
        span_name: Option<String>,
    ) -> Self {
        let text = format_db_bytes(&bytes);
        Self {
            offset,
            virtual_address,
            assembly_text: text.clone(),
            text,
            bytes: RowBytes::from_slice(&bytes),
            symbolized_names: Vec::new(),
            symbol_label,
            direct_target: None,
            function_scope: None,
            span_name,
            kind: DisasmRowKind::Data,
        }
    }

    pub fn invalid(
        offset: u64,
        virtual_address: Option<u64>,
        byte: u8,
        symbol_label: Option<String>,
        span_name: Option<String>,
    ) -> Self {
        let bytes = RowBytes::from_slice(&[byte]);
        let text = format_db_bytes(bytes.as_slice());
        Self {
            offset,
            virtual_address,
            bytes,
            assembly_text: text.clone(),
            text,
            symbolized_names: Vec::new(),
            symbol_label,
            direct_target: None,
            function_scope: None,
            span_name,
            kind: DisasmRowKind::Invalid,
        }
    }

    pub fn len(&self) -> usize {
        self.bytes.len().max(1)
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn label(&self) -> String {
        match &self.span_name {
            Some(name) => format!("{name}:0x{:x}", self.offset),
            None => format!("<raw>:0x{:x}", self.offset),
        }
    }
}

fn format_db_bytes(bytes: &[u8]) -> String {
    let body = bytes
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(".db {body}")
}
