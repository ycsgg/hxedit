use yaxpeax_arch::{Decoder, U8Reader};
use yaxpeax_arm::armv8::a64::{InstDecoder, Opcode, Operand};

use crate::disasm::backend::{BackendKind, DisassemblerBackend};
use crate::disasm::types::{DecodedInstruction, DirectBranchKind, DirectBranchTarget};
use crate::error::{HxError, HxResult};
use crate::executable::{ExecutableArch, ExecutableInfo};

const INSTRUCTION_BYTES: usize = 4;

pub struct YaxpeaxArmBackend {
    decoder: InstDecoder,
}

impl YaxpeaxArmBackend {
    pub fn new(info: &ExecutableInfo) -> HxResult<Self> {
        if !Self::supports_arch(info.arch) {
            return Err(HxError::DisassemblyUnavailable(format!(
                "yaxpeax-arm does not support arch {}",
                info.arch.label()
            )));
        }
        Ok(Self {
            decoder: InstDecoder::default(),
        })
    }

    pub fn supports_arch(arch: ExecutableArch) -> bool {
        matches!(arch, ExecutableArch::AArch64)
    }
}

impl DisassemblerBackend for YaxpeaxArmBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::YaxpeaxArm
    }

    fn name(&self) -> &'static str {
        "yaxpeax-arm"
    }

    fn max_instruction_bytes(&self) -> usize {
        INSTRUCTION_BYTES
    }

    fn decode_one(&self, address: u64, bytes: &[u8]) -> HxResult<Option<DecodedInstruction>> {
        if bytes.len() < INSTRUCTION_BYTES {
            return Ok(None);
        }
        let word = &bytes[..INSTRUCTION_BYTES];
        let mut reader = U8Reader::new(word);
        let Ok(instruction) = self.decoder.decode(&mut reader) else {
            return Ok(None);
        };

        // yaxpeax renders PC-relative operands as `$±0xN`; rewrite them to the
        // absolute virtual address so the shared symbolization pipeline (and the
        // capstone-style display) can resolve them.
        let raw_text = format!("{instruction}");
        let text = rewrite_pc_relative(&raw_text, address);
        let direct_target = direct_branch_target(&instruction, address);

        Ok(Some(DecodedInstruction {
            bytes: word.to_vec(),
            text,
            direct_target,
        }))
    }
}

fn direct_branch_target(
    instruction: &yaxpeax_arm::armv8::a64::Instruction,
    address: u64,
) -> Option<DirectBranchTarget> {
    let kind = match instruction.opcode {
        Opcode::BL | Opcode::BLR => DirectBranchKind::Call,
        Opcode::B
        | Opcode::Bcc(_)
        | Opcode::CBZ
        | Opcode::CBNZ
        | Opcode::TBZ
        | Opcode::TBNZ
        | Opcode::BR => DirectBranchKind::Jump,
        _ => return None,
    };
    let offset = instruction.operands.iter().find_map(|op| match op {
        Operand::PCOffset(rel) => Some(*rel),
        _ => None,
    })?;
    let virtual_address = (address as i64).wrapping_add(offset) as u64;
    Some(DirectBranchTarget {
        kind,
        virtual_address,
        display_name: None,
    })
}

/// Replace yaxpeax's `$+0xN` / `$-0xN` PC-relative tokens with the absolute
/// virtual address `0x...`, computed from `address`.
fn rewrite_pc_relative(text: &str, address: u64) -> String {
    let Some(start) = text.find('$') else {
        return text.to_owned();
    };
    let rest = &text[start + 1..];
    let (sign, sign_len) = match rest.as_bytes().first() {
        Some(b'+') => (1i64, 1usize),
        Some(b'-') => (-1i64, 1usize),
        _ => (1i64, 0usize),
    };
    let after_sign = &rest[sign_len..];
    let (prefix_len, hex) = match after_sign.strip_prefix("0x") {
        Some(hex) => (2usize, hex),
        None => (0usize, after_sign),
    };
    let digits: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    let Ok(magnitude) = i64::from_str_radix(&digits, 16) else {
        return text.to_owned();
    };
    let consumed = 1 + sign_len + prefix_len + digits.len();
    let absolute = (address as i64).wrapping_add(sign * magnitude) as u64;
    format!("{}0x{absolute:x}{}", &text[..start], &text[start + consumed..])
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
    fn decodes_four_byte_aarch64_instructions() {
        let backend = YaxpeaxArmBackend::new(&info(ExecutableArch::AArch64)).unwrap();
        // ret = 0xd65f03c0 (little-endian bytes)
        let ret = backend
            .decode_one(0x1000, &[0xc0, 0x03, 0x5f, 0xd6])
            .unwrap()
            .unwrap();
        assert_eq!(ret.bytes.len(), 4);
        assert!(ret.text.contains("ret"));
    }

    #[test]
    fn resolves_bl_call_target_and_absolute_text() {
        let backend = YaxpeaxArmBackend::new(&info(ExecutableArch::AArch64)).unwrap();
        // bl #0 at 0x1004 -> target 0x1004 (0x94000000)
        let row = backend
            .decode_one(0x1004, &[0x00, 0x00, 0x00, 0x94])
            .unwrap()
            .unwrap();
        let target = row.direct_target.expect("bl target");
        assert_eq!(target.kind, DirectBranchKind::Call);
        assert_eq!(target.virtual_address, 0x1004);
        // PC-relative token rewritten to absolute VA.
        assert!(row.text.contains("0x1004"), "text was: {}", row.text);
        assert!(!row.text.contains('$'), "text was: {}", row.text);
    }

    #[test]
    fn rewrite_pc_relative_handles_positive_and_negative() {
        assert_eq!(rewrite_pc_relative("b.lt $+0x30", 0x1c), "b.lt 0x4c");
        assert_eq!(rewrite_pc_relative("b.ne $-0x18", 0x40), "b.ne 0x28");
        assert_eq!(rewrite_pc_relative("ret", 0x10), "ret");
    }
}
