use crate::disasm::backend::BackendKind;
use crate::executable::ExecutableInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisassemblyState {
    pub info: ExecutableInfo,
    pub backend: BackendKind,
    pub viewport_top: u64,
    pub last_error: Option<String>,
}

impl DisassemblyState {
    pub fn new(info: ExecutableInfo, backend: BackendKind, viewport_top: u64) -> Self {
        Self {
            info,
            backend,
            viewport_top,
            last_error: None,
        }
    }
}
