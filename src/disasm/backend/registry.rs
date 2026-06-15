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
/// x86/x86_64 prefer iced-x86, AArch64 prefers yaxpeax-arm; capstone is the
/// fallback for those arches when the dedicated backend is not compiled, and the
/// only option for arm/riscv64.
fn default_kind_for_arch(arch: ExecutableArch) -> Option<BackendKind> {
    match arch {
        ExecutableArch::X86 | ExecutableArch::X86_64 => {
            if cfg!(feature = "disasm-iced-x86") {
                Some(BackendKind::IcedX86)
            } else if capstone_supports(arch) {
                Some(BackendKind::Capstone)
            } else {
                None
            }
        }
        ExecutableArch::AArch64 => {
            if cfg!(feature = "disasm-yaxpeax-arm") {
                Some(BackendKind::YaxpeaxArm)
            } else if capstone_supports(arch) {
                Some(BackendKind::Capstone)
            } else {
                None
            }
        }
        _ => capstone_supports(arch).then_some(BackendKind::Capstone),
    }
}

fn backend_supports(kind: BackendKind, arch: ExecutableArch) -> bool {
    match kind {
        BackendKind::Capstone => capstone_supports(arch),
        BackendKind::IcedX86 => {
            cfg!(feature = "disasm-iced-x86")
                && matches!(arch, ExecutableArch::X86 | ExecutableArch::X86_64)
        }
        BackendKind::YaxpeaxArm => {
            cfg!(feature = "disasm-yaxpeax-arm") && matches!(arch, ExecutableArch::AArch64)
        }
    }
}

fn capstone_supports(arch: ExecutableArch) -> bool {
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
