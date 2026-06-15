use crate::disasm::backend::{BackendKind, DisassemblerBackend};
use crate::error::{HxError, HxResult};
use crate::executable::{ExecutableArch, ExecutableInfo};

pub fn resolve_backend_kind(
    info: &ExecutableInfo,
    preferred: Option<BackendKind>,
) -> HxResult<BackendKind> {
    if let Some(kind) = preferred {
        if backend_supports(kind, info.arch) {
            return Ok(kind);
        }
        return Err(HxError::DisassemblyUnavailable(format!(
            "{} backend does not support arch {}",
            kind.label(),
            info.arch.label()
        )));
    }

    default_kind_for_arch(info.arch).ok_or_else(|| {
        HxError::DisassemblyUnavailable(format!("unsupported arch {}", info.arch.label()))
    })
}

pub fn resolve_backend(
    info: &ExecutableInfo,
    preferred: Option<BackendKind>,
) -> HxResult<Box<dyn DisassemblerBackend>> {
    let kind = resolve_backend_kind(info, preferred)?;
    match kind {
        BackendKind::IcedX86 => {
            #[cfg(feature = "disasm-iced-x86")]
            {
                Ok(Box::new(crate::disasm::backend::IcedX86Backend::new(info)?))
            }
            #[cfg(not(feature = "disasm-iced-x86"))]
            {
                let _ = info;
                Err(HxError::DisassemblyUnavailable(
                    "iced-x86 backend is not enabled in this build".to_owned(),
                ))
            }
        }
        BackendKind::YaxpeaxArm => {
            #[cfg(feature = "disasm-yaxpeax-arm")]
            {
                Ok(Box::new(crate::disasm::backend::YaxpeaxArmBackend::new(
                    info,
                )?))
            }
            #[cfg(not(feature = "disasm-yaxpeax-arm"))]
            {
                let _ = info;
                Err(HxError::DisassemblyUnavailable(
                    "yaxpeax-arm backend is not enabled in this build".to_owned(),
                ))
            }
        }
    }
}

/// Pick the preferred backend for an arch given the compiled-in features.
///
/// x86/x86_64 decode with iced-x86, AArch64 with yaxpeax-arm. Other arches have
/// no pure-Rust backend available.
fn default_kind_for_arch(arch: ExecutableArch) -> Option<BackendKind> {
    match arch {
        ExecutableArch::X86 | ExecutableArch::X86_64 => {
            cfg!(feature = "disasm-iced-x86").then_some(BackendKind::IcedX86)
        }
        ExecutableArch::AArch64 => {
            cfg!(feature = "disasm-yaxpeax-arm").then_some(BackendKind::YaxpeaxArm)
        }
        _ => None,
    }
}

fn backend_supports(kind: BackendKind, arch: ExecutableArch) -> bool {
    match kind {
        BackendKind::IcedX86 => {
            cfg!(feature = "disasm-iced-x86")
                && matches!(arch, ExecutableArch::X86 | ExecutableArch::X86_64)
        }
        BackendKind::YaxpeaxArm => {
            cfg!(feature = "disasm-yaxpeax-arm") && matches!(arch, ExecutableArch::AArch64)
        }
    }
}
