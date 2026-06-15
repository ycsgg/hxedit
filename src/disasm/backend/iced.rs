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
        let mut formatter = FastFormatter::new();
        formatter.options_mut().set_use_hex_prefix(true);
        Ok(decoded_from(&instruction, bytes, &mut formatter))
    }

    fn decode_block(
        &self,
        address: u64,
        bytes: &[u8],
        max_instructions: usize,
        out: &mut Vec<DecodedInstruction>,
    ) -> HxResult<()> {
        if bytes.is_empty() || max_instructions == 0 {
            return Ok(());
        }
        // A single streaming decoder reuses internal state across the whole
        // buffer, which is dramatically faster than constructing one per
        // instruction. Stop at the first byte that does not decode cleanly so
        // the caller can fall back to its per-row / seam handling.
        let mut decoder = Decoder::with_ip(self.bitness, bytes, address, DecoderOptions::NONE);
        let mut formatter = FastFormatter::new();
        formatter.options_mut().set_use_hex_prefix(true);
        let mut instruction = Instruction::default();
        while out.len() < max_instructions && decoder.can_decode() {
            let pos = decoder.position();
            decoder.decode_out(&mut instruction);
            match decoded_from(&instruction, &bytes[pos..], &mut formatter) {
                Some(decoded) => out.push(decoded),
                None => break,
            }
        }
        Ok(())
    }
}

/// Build a `DecodedInstruction` from an already-decoded iced instruction,
/// validating length against the remaining buffer. Returns `None` for invalid
/// or truncated instructions so callers can fall back to `.db` handling.
fn decoded_from(
    instruction: &Instruction,
    remaining: &[u8],
    formatter: &mut FastFormatter,
) -> Option<DecodedInstruction> {
    if instruction.is_invalid() {
        return None;
    }
    let len = instruction.len();
    if len == 0 || len > remaining.len() {
        return None;
    }
    let mut text = String::new();
    formatter.format(instruction, &mut text);
    Some(DecodedInstruction {
        bytes: remaining[..len].to_vec(),
        text,
        direct_target: direct_branch_target(instruction),
    })
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
