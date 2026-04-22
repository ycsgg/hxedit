use capstone::prelude::*;
use capstone::{arch::DetailsArchInsn, InsnGroupType};

use crate::disasm::backend::{BackendKind, DisassemblerBackend};
use crate::disasm::types::{DecodedInstruction, DirectBranchKind, DirectBranchTarget};
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

    fn decode_one(&self, address: u64, bytes: &[u8]) -> HxResult<Option<DecodedInstruction>> {
        if bytes.is_empty() {
            return Ok(None);
        }
        let instructions = self
            .engine
            .disasm_count(bytes, address, 1)
            .map_err(capstone_error)?;
        let Some(insn) = instructions.as_ref().first() else {
            return Ok(None);
        };
        let text = format_instruction_text(insn);
        let direct_target = self.extract_direct_branch_target(insn);
        Ok(Some(DecodedInstruction {
            bytes: insn.bytes().to_vec(),
            text,
            direct_target,
        }))
    }
}

impl CapstoneBackend {
    fn extract_direct_branch_target(
        &self,
        insn: &capstone::Insn<'_>,
    ) -> Option<DirectBranchTarget> {
        let detail = self.engine.insn_detail(insn).ok()?;
        let kind = branch_kind_from_groups(&detail)?;
        match detail.arch_detail() {
            capstone::arch::ArchDetail::X86Detail(detail) => {
                extract_x86_direct_target(&detail, kind)
            }
            capstone::arch::ArchDetail::Arm64Detail(detail) => {
                extract_arm64_direct_target(&detail, kind)
            }
        }
    }
}

fn branch_kind_from_groups(detail: &capstone::InsnDetail<'_>) -> Option<DirectBranchKind> {
    detail.groups().iter().find_map(|group| {
        if group.0 == InsnGroupType::CS_GRP_CALL as u8 {
            Some(DirectBranchKind::Call)
        } else if group.0 == InsnGroupType::CS_GRP_JUMP as u8 {
            Some(DirectBranchKind::Jump)
        } else {
            None
        }
    })
}

fn extract_x86_direct_target(
    detail: &capstone::arch::x86::X86InsnDetail<'_>,
    kind: DirectBranchKind,
) -> Option<DirectBranchTarget> {
    let operand = detail.operands().next()?;
    let capstone::arch::x86::X86OperandType::Imm(target) = operand.op_type else {
        return None;
    };
    Some(DirectBranchTarget {
        kind,
        virtual_address: u64::try_from(target).ok()?,
        display_name: None,
    })
}

fn extract_arm64_direct_target(
    detail: &capstone::arch::arm64::Arm64InsnDetail<'_>,
    kind: DirectBranchKind,
) -> Option<DirectBranchTarget> {
    let target = detail
        .operands()
        .filter_map(|operand| match operand.op_type {
            capstone::arch::arm64::Arm64OperandType::Imm(target) => u64::try_from(target).ok(),
            _ => None,
        });
    Some(DirectBranchTarget {
        kind,
        virtual_address: target.last()?,
        display_name: None,
    })
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
