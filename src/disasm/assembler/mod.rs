mod registry;
mod symbols;
mod traits;

#[cfg(feature = "asm")]
mod keystone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblerKind {
    Keystone,
}

impl AssemblerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Keystone => "keystone",
        }
    }
}

#[cfg(feature = "asm")]
pub use keystone::KeystoneBackend;
pub use registry::resolve_assembler_backend;
pub use symbols::{resolve_patch_symbols, ResolvedAssemblyStatement, ResolvedPatchSymbol};
pub use traits::AssemblerBackend;
