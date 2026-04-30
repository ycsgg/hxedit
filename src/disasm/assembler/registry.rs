use crate::disasm::assembler::{AssemblerBackend, AssemblerKind};
use crate::error::{HxError, HxResult};
use crate::executable::ExecutableInfo;

pub fn resolve_assembler_backend(
    info: &ExecutableInfo,
    preferred: Option<AssemblerKind>,
) -> HxResult<Box<dyn AssemblerBackend>> {
    let kind = preferred.unwrap_or(AssemblerKind::Keystone);
    match kind {
        AssemblerKind::Keystone => {
            #[cfg(feature = "asm")]
            {
                let backend = crate::disasm::assembler::KeystoneBackend::new();
                if backend.supports_arch(info.arch, info.bitness, info.endian) {
                    Ok(Box::new(backend))
                } else {
                    Err(HxError::DisassemblyUnavailable(format!(
                        "assembly patch unavailable: keystone backend does not support {} {} {}",
                        info.arch.label(),
                        info.bitness.label(),
                        info.endian.label()
                    )))
                }
            }
            #[cfg(not(feature = "asm"))]
            {
                let _ = info;
                Err(HxError::DisassemblyUnavailable(
                    "assembly patch unavailable: keystone backend is not enabled in this build"
                        .to_owned(),
                ))
            }
        }
    }
}
