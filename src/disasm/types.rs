#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub bytes: Vec<u8>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisasmRow {
    pub offset: u64,
    pub virtual_address: Option<u64>,
    pub bytes: Vec<u8>,
    pub text: String,
    pub symbolized_names: Vec<String>,
    pub symbol_label: Option<String>,
    pub direct_target: Option<DirectBranchTarget>,
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
        Self {
            offset,
            virtual_address,
            text: format_db_bytes(&bytes),
            bytes,
            symbolized_names: Vec::new(),
            symbol_label,
            direct_target: None,
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
        Self {
            offset,
            virtual_address,
            bytes: vec![byte],
            text: format_db_bytes(&[byte]),
            symbolized_names: Vec::new(),
            symbol_label,
            direct_target: None,
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
