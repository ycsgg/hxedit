use iced_x86::{Decoder, DecoderOptions, FastFormatter, Instruction, Mnemonic, OpKind};

use crate::disasm::backend::{BackendKind, DisassemblerBackend};
use crate::disasm::types::{DecodedInstruction, DirectBranchKind, DirectBranchTarget};
use crate::error::{HxError, HxResult};
use crate::executable::{ExecutableArch, ExecutableInfo};

pub struct IcedX86Backend {
    bitness: u32,
}

impl IcedX86Backend {
    pub fn new(info: &ExecutableInfo) -> HxResult<Self> {
        let bitness = match info.arch {
            ExecutableArch::X86 => 32,
            ExecutableArch::X86_64 => 64,
            _ => {
                return Err(HxError::DisassemblyUnavailable(format!(
                    "iced-x86 does not support arch {}",
                    info.arch.label()
                )))
            }
        };
        Ok(Self { bitness })
    }

    pub fn supports_arch(arch: ExecutableArch) -> bool {
        matches!(arch, ExecutableArch::X86 | ExecutableArch::X86_64)
    }
}

impl DisassemblerBackend for IcedX86Backend {
    fn kind(&self) -> BackendKind {
        BackendKind::IcedX86
    }

    fn name(&self) -> &'static str {
        "iced-x86"
    }

    fn max_instruction_bytes(&self) -> usize {
        15
    }

    fn decode_one(&self, address: u64, bytes: &[u8]) -> HxResult<Option<DecodedInstruction>> {
        if bytes.is_empty() {
            return Ok(None);
        }
        let mut decoder = Decoder::with_ip(self.bitness, bytes, address, DecoderOptions::NONE);
        if !decoder.can_decode() {
            return Ok(None);
        }
        let mut instruction = Instruction::default();
        decoder.decode_out(&mut instruction);
        if instruction.is_invalid() {
            return Ok(None);
        }
        let len = instruction.len();
        if len == 0 || len > bytes.len() {
            return Ok(None);
        }

        let mut text = String::new();
        let mut formatter = FastFormatter::new();
        formatter.options_mut().set_use_hex_prefix(true);
        formatter.format(&instruction, &mut text);

        let direct_target = direct_branch_target(&instruction);

        Ok(Some(DecodedInstruction {
            bytes: bytes[..len].to_vec(),
            text,
            direct_target,
        }))
    }
}

fn direct_branch_target(instruction: &Instruction) -> Option<DirectBranchTarget> {
    // Only direct (PC-relative) branches carry a resolvable target; indirect
    // register/memory branches use other operand kinds.
    if !matches!(
        instruction.op0_kind(),
        OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
    ) {
        return None;
    }
    let kind = match instruction.mnemonic() {
        Mnemonic::Call => DirectBranchKind::Call,
        Mnemonic::Jmp
        | Mnemonic::Ja
        | Mnemonic::Jae
        | Mnemonic::Jb
        | Mnemonic::Jbe
        | Mnemonic::Jcxz
        | Mnemonic::Je
        | Mnemonic::Jecxz
        | Mnemonic::Jg
        | Mnemonic::Jge
        | Mnemonic::Jl
        | Mnemonic::Jle
        | Mnemonic::Jne
        | Mnemonic::Jno
        | Mnemonic::Jnp
        | Mnemonic::Jns
        | Mnemonic::Jo
        | Mnemonic::Jp
        | Mnemonic::Jrcxz
        | Mnemonic::Js
        | Mnemonic::Loop
        | Mnemonic::Loope
        | Mnemonic::Loopne => DirectBranchKind::Jump,
        _ => return None,
    };
    Some(DirectBranchTarget {
        kind,
        virtual_address: instruction.near_branch_target(),
        display_name: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executable::{
        Bitness, Endian, ExecutableArch, ExecutableInfo, ExecutableKind,
    };
    use std::collections::{BTreeMap, HashMap};

    fn info(arch: ExecutableArch) -> ExecutableInfo {
        ExecutableInfo {
            kind: ExecutableKind::Raw,
            arch,
            bitness: Bitness::Bit64,
            endian: Endian::Little,
            entry_offset: None,
            entry_virtual_address: None,
            code_spans: Vec::new(),
            symbols_by_va: BTreeMap::new(),
            target_names_by_va: Box::new(BTreeMap::new()),
            symbols_by_name: HashMap::new(),
            target_names_by_name: HashMap::new(),
            imports: Vec::new(),
        }
    }

    #[test]
    fn decodes_x86_64_instructions_with_lengths() {
        let backend = IcedX86Backend::new(&info(ExecutableArch::X86_64)).unwrap();
        // push rbp; mov rbp, rsp; nop; ret
        let bytes = [0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3];
        let push = backend.decode_one(0x1000, &bytes).unwrap().unwrap();
        assert_eq!(push.bytes, vec![0x55]);
        assert!(push.text.contains("push"));
        let mov = backend.decode_one(0x1001, &bytes[1..]).unwrap().unwrap();
        assert_eq!(mov.bytes.len(), 3);
        assert!(mov.text.contains("mov"));
    }

    #[test]
    fn resolves_direct_call_target() {
        let backend = IcedX86Backend::new(&info(ExecutableArch::X86_64)).unwrap();
        // call rel32 = -6 from address 0x1000 (next ip 0x1005) -> 0xfff
        let bytes = [0xe8, 0xfa, 0xff, 0xff, 0xff];
        let row = backend.decode_one(0x1000, &bytes).unwrap().unwrap();
        let target = row.direct_target.expect("call target");
        assert_eq!(target.kind, DirectBranchKind::Call);
        assert_eq!(target.virtual_address, 0xfff);
    }

    #[test]
    fn invalid_bytes_decode_to_none_or_db() {
        let backend = IcedX86Backend::new(&info(ExecutableArch::X86_64)).unwrap();
        // 0xf4 = hlt is valid; use a lone 0x0f which needs a second byte.
        let row = backend.decode_one(0x1000, &[0x0f]).unwrap();
        assert!(row.is_none());
    }
}
