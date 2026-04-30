use keystone_engine::{Arch, Keystone, Mode, OptionType, OptionValue};

use crate::disasm::assembler::{AssemblerBackend, AssemblerKind};
use crate::error::{HxError, HxResult};
use crate::executable::{Bitness, Endian, ExecutableArch};

#[derive(Debug, Default)]
pub struct KeystoneBackend;

impl KeystoneBackend {
    pub fn new() -> Self {
        Self
    }

    fn mode_for(arch: ExecutableArch, bitness: Bitness, endian: Endian) -> HxResult<Mode> {
        if matches!(endian, Endian::Big) {
            return Err(HxError::DisassemblyUnavailable(
                "assembly patch unavailable: big-endian keystone mode is not wired yet".to_owned(),
            ));
        }

        match arch {
            ExecutableArch::X86 => Ok(match bitness {
                Bitness::Bit32 => Mode::MODE_32,
                Bitness::Bit64 => Mode::MODE_64,
            }),
            ExecutableArch::X86_64 => Ok(Mode::MODE_64),
            ExecutableArch::AArch64 => Ok(Mode::LITTLE_ENDIAN),
            _ => Err(HxError::DisassemblyUnavailable(format!(
                "assembly patch unavailable: unsupported arch {}",
                arch.label()
            ))),
        }
    }

    fn arch_for(arch: ExecutableArch) -> HxResult<Arch> {
        match arch {
            ExecutableArch::X86 | ExecutableArch::X86_64 => Ok(Arch::X86),
            ExecutableArch::AArch64 => Ok(Arch::ARM64),
            _ => Err(HxError::DisassemblyUnavailable(format!(
                "assembly patch unavailable: unsupported arch {}",
                arch.label()
            ))),
        }
    }
}

impl AssemblerBackend for KeystoneBackend {
    fn kind(&self) -> AssemblerKind {
        AssemblerKind::Keystone
    }

    fn name(&self) -> &'static str {
        "keystone"
    }

    fn supports_arch(&self, arch: ExecutableArch, bitness: Bitness, endian: Endian) -> bool {
        Self::arch_for(arch).is_ok() && Self::mode_for(arch, bitness, endian).is_ok()
    }

    fn assemble_one(
        &self,
        arch: ExecutableArch,
        bitness: Bitness,
        endian: Endian,
        address: u64,
        statement: &str,
    ) -> HxResult<Vec<u8>> {
        let statement = statement.trim();
        if statement.is_empty() {
            return Err(HxError::AssemblyError(
                "instruction must not be empty".to_owned(),
            ));
        }

        let engine = Keystone::new(
            Self::arch_for(arch)?,
            Self::mode_for(arch, bitness, endian)?,
        )
        .map_err(|err| HxError::DisassemblyUnavailable(format!("keystone init failed: {err}")))?;
        if matches!(arch, ExecutableArch::X86 | ExecutableArch::X86_64) {
            engine
                .option(OptionType::SYNTAX, OptionValue::SYNTAX_INTEL)
                .map_err(|err| {
                    HxError::DisassemblyUnavailable(format!("keystone syntax setup failed: {err}"))
                })?;
        }

        let output = engine
            .asm(statement.to_owned(), address)
            .map_err(|err| HxError::AssemblyError(err.to_string()))?;
        if output.bytes.is_empty() {
            return Err(HxError::AssemblyError(
                "assembler produced no bytes".to_owned(),
            ));
        }
        Ok(output.bytes)
    }
}
