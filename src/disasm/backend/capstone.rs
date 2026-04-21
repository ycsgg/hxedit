use capstone::prelude::*;

use crate::disasm::backend::{BackendKind, DisassemblerBackend};
use crate::disasm::types::DecodedInstruction;
use crate::error::{HxError, HxResult};
use crate::executable::{Endian, ExecutableArch, ExecutableInfo};

pub struct CapstoneBackend {
    engine: capstone::Capstone,
    max_instruction_bytes: usize,
}

impl CapstoneBackend {
    pub fn new(info: &ExecutableInfo) -> HxResult<Self> {
        match info.arch {
            ExecutableArch::X86 => {
                let engine = Capstone::new()
                    .x86()
                    .mode(arch::x86::ArchMode::Mode32)
                    .syntax(arch::x86::ArchSyntax::Intel)
                    .detail(true)
                    .build()
                    .map_err(capstone_error)?;
                Ok(Self {
                    engine,
                    max_instruction_bytes: 15,
                })
            }
            ExecutableArch::X86_64 => {
                let engine = Capstone::new()
                    .x86()
                    .mode(arch::x86::ArchMode::Mode64)
                    .syntax(arch::x86::ArchSyntax::Intel)
                    .detail(true)
                    .build()
                    .map_err(capstone_error)?;
                Ok(Self {
                    engine,
                    max_instruction_bytes: 15,
                })
            }
            ExecutableArch::AArch64 => {
                let builder = Capstone::new()
                    .arm64()
                    .mode(arch::arm64::ArchMode::Arm)
                    .detail(true);
                let engine = match info.endian {
                    Endian::Little => builder
                        .endian(capstone::Endian::Little)
                        .build()
                        .map_err(capstone_error)?,
                    Endian::Big => builder
                        .endian(capstone::Endian::Big)
                        .build()
                        .map_err(capstone_error)?,
                };
                Ok(Self {
                    engine,
                    max_instruction_bytes: 4,
                })
            }
            _ => Err(HxError::DisassemblyUnavailable(format!(
                "unsupported arch {}",
                info.arch.label()
            ))),
        }
    }

    pub fn supports_arch(arch: ExecutableArch) -> bool {
        matches!(
            arch,
            ExecutableArch::X86 | ExecutableArch::X86_64 | ExecutableArch::AArch64
        )
    }
}

impl DisassemblerBackend for CapstoneBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Capstone
    }

    fn name(&self) -> &'static str {
        "capstone"
    }

    fn max_instruction_bytes(&self) -> usize {
        self.max_instruction_bytes
    }

    fn decode_one(&self, offset: u64, bytes: &[u8]) -> HxResult<Option<DecodedInstruction>> {
        if bytes.is_empty() {
            return Ok(None);
        }
        let instructions = self
            .engine
            .disasm_count(bytes, offset, 1)
            .map_err(capstone_error)?;
        let Some(insn) = instructions.as_ref().first() else {
            return Ok(None);
        };
        let text = format_instruction_text(insn);
        Ok(Some(DecodedInstruction {
            bytes: insn.bytes().to_vec(),
            text,
        }))
    }
}

fn format_instruction_text(insn: &capstone::Insn<'_>) -> String {
    let mnemonic = insn.mnemonic().unwrap_or("<?>");
    let op_str = insn.op_str().unwrap_or("");
    if op_str.is_empty() {
        mnemonic.to_owned()
    } else {
        format!("{mnemonic} {op_str}")
    }
}

fn capstone_error(err: capstone::Error) -> HxError {
    HxError::DisassemblyUnavailable(format!("capstone decode failed: {err}"))
}
