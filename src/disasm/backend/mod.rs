mod registry;
mod traits;

#[cfg(feature = "disasm-capstone")]
mod capstone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Capstone,
    IcedX86,
}

impl BackendKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Capstone => "capstone",
            Self::IcedX86 => "iced-x86",
        }
    }
}

#[cfg(feature = "disasm-capstone")]
pub use capstone::CapstoneBackend;
pub use registry::{resolve_backend, resolve_backend_kind};
pub use traits::DisassemblerBackend;
