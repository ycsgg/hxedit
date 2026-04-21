use crate::disasm::backend::{BackendKind, DisassemblerBackend};
use crate::error::{HxError, HxResult};
use crate::executable::{ExecutableArch, ExecutableInfo};

pub fn resolve_backend_kind(
    info: &ExecutableInfo,
    preferred: Option<BackendKind>,
) -> HxResult<BackendKind> {
    let kind = preferred.unwrap_or(BackendKind::Capstone);
    if backend_supports(kind, info.arch) {
        Ok(kind)
    } else {
        Err(HxError::DisassemblyUnavailable(format!(
            "unsupported arch {}",
            info.arch.label()
        )))
    }
}

pub fn resolve_backend(
    info: &ExecutableInfo,
    preferred: Option<BackendKind>,
) -> HxResult<Box<dyn DisassemblerBackend>> {
    let kind = resolve_backend_kind(info, preferred)?;
    match kind {
        BackendKind::Capstone => {
            #[cfg(feature = "disasm-capstone")]
            {
                Ok(Box::new(crate::disasm::backend::CapstoneBackend::new(
                    info,
                )?))
            }
            #[cfg(not(feature = "disasm-capstone"))]
            {
                let _ = info;
                Err(HxError::DisassemblyUnavailable(
                    "capstone backend is not enabled in this build".to_owned(),
                ))
            }
        }
        BackendKind::IcedX86 => Err(HxError::DisassemblyUnavailable(
            "iced-x86 backend is not implemented yet".to_owned(),
        )),
    }
}

fn backend_supports(kind: BackendKind, arch: ExecutableArch) -> bool {
    match kind {
        BackendKind::Capstone => {
            #[cfg(feature = "disasm-capstone")]
            {
                crate::disasm::backend::CapstoneBackend::supports_arch(arch)
            }
            #[cfg(not(feature = "disasm-capstone"))]
            {
                let _ = arch;
                false
            }
        }
        BackendKind::IcedX86 => false,
    }
}
