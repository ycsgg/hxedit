mod registry;
mod traits;

#[cfg(feature = "disasm-capstone")]
mod capstone;
#[cfg(feature = "disasm-iced-x86")]
mod iced;
#[cfg(feature = "disasm-yaxpeax-arm")]
mod yaxpeax;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Capstone,
    IcedX86,
    YaxpeaxArm,
}

impl BackendKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Capstone => "capstone",
            Self::IcedX86 => "iced-x86",
            Self::YaxpeaxArm => "yaxpeax-arm",
        }
    }
}

#[cfg(feature = "disasm-capstone")]
pub use capstone::CapstoneBackend;
#[cfg(feature = "disasm-iced-x86")]
pub use iced::IcedX86Backend;
#[cfg(feature = "disasm-yaxpeax-arm")]
pub use yaxpeax::YaxpeaxArmBackend;
pub use registry::{resolve_backend, resolve_backend_kind};
pub use traits::DisassemblerBackend;
