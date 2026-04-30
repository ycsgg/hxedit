use crate::disasm::{DisasmRow, DisasmRowKind};
use crate::error::{HxError, HxResult};
use crate::executable::ExecutableArch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssemblyPatchPlan {
    pub patch_bytes: Vec<u8>,
    pub patch_len: usize,
    pub original_len: usize,
    pub covered_rows: usize,
    pub trailing_nop_len: usize,
}

pub fn plan_assembly_patch(
    rows: &[DisasmRow],
    assembled: &[u8],
    arch: ExecutableArch,
) -> HxResult<AssemblyPatchPlan> {
    let Some(first) = rows.first() else {
        return Err(HxError::AssemblyError(
            "no disassembly row available at cursor".to_owned(),
        ));
    };
    if first.kind != DisasmRowKind::Instruction {
        return Err(HxError::AssemblyError(
            "assembly patch only applies to instruction rows".to_owned(),
        ));
    }
    if assembled.is_empty() {
        return Err(HxError::AssemblyError(
            "assembler produced no bytes".to_owned(),
        ));
    }

    let original_len = first.len();
    if assembled.len() <= original_len {
        let trailing_nop_len = original_len.saturating_sub(assembled.len());
        let mut patch_bytes = assembled.to_vec();
        patch_bytes.extend(nop_bytes(arch, trailing_nop_len)?);
        return Ok(AssemblyPatchPlan {
            patch_bytes,
            patch_len: original_len,
            original_len,
            covered_rows: 1,
            trailing_nop_len,
        });
    }

    let mut covered = original_len;
    let mut covered_rows = 1usize;
    while covered < assembled.len() {
        let Some(next) = rows.get(covered_rows) else {
            return Err(HxError::AssemblyError(
                "assembled instruction exceeds remaining bytes in current disassembly region"
                    .to_owned(),
            ));
        };
        covered = covered.saturating_add(next.len());
        covered_rows += 1;
    }

    let trailing_nop_len = covered.saturating_sub(assembled.len());
    let mut patch_bytes = assembled.to_vec();
    patch_bytes.extend(nop_bytes(arch, trailing_nop_len)?);
    Ok(AssemblyPatchPlan {
        patch_bytes,
        patch_len: covered,
        original_len,
        covered_rows,
        trailing_nop_len,
    })
}

fn nop_bytes(arch: ExecutableArch, len: usize) -> HxResult<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    match arch {
        ExecutableArch::X86 | ExecutableArch::X86_64 => Ok(vec![0x90; len]),
        ExecutableArch::AArch64 => {
            if !len.is_multiple_of(4) {
                return Err(HxError::AssemblyError(format!(
                    "aarch64 nop fill requires 4-byte alignment, got {len} trailing bytes"
                )));
            }
            let nop = [0x1f, 0x20, 0x03, 0xd5];
            let mut out = Vec::with_capacity(len);
            for _ in 0..(len / 4) {
                out.extend_from_slice(&nop);
            }
            Ok(out)
        }
        _ => Err(HxError::DisassemblyUnavailable(format!(
            "assembly patch unavailable: unsupported arch {}",
            arch.label()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_assembly_patch, AssemblyPatchPlan};
    use crate::disasm::{DisasmRow, DisasmRowKind};
    use crate::executable::ExecutableArch;

    fn row(offset: u64, bytes: &[u8]) -> DisasmRow {
        DisasmRow {
            offset,
            virtual_address: Some(0x401000 + offset),
            bytes: bytes.to_vec(),
            text: "nop".to_owned(),
            assembly_text: "nop".to_owned(),
            symbolized_names: Vec::new(),
            symbol_label: None,
            direct_target: None,
            span_name: Some(".text".to_owned()),
            kind: DisasmRowKind::Instruction,
        }
    }

    #[test]
    fn shorter_x86_patch_nop_fills_current_instruction() {
        let rows = vec![row(0x100, &[0x48, 0x83, 0xec, 0x20])];
        let plan = plan_assembly_patch(&rows, &[0x90], ExecutableArch::X86_64).unwrap();
        assert_eq!(
            plan,
            AssemblyPatchPlan {
                patch_bytes: vec![0x90, 0x90, 0x90, 0x90],
                patch_len: 4,
                original_len: 4,
                covered_rows: 1,
                trailing_nop_len: 3,
            }
        );
    }

    #[test]
    fn longer_x86_patch_extends_to_row_boundary_and_nops_tail() {
        let rows = vec![
            row(0x100, &[0x90, 0x90, 0x90]),
            row(0x103, &[0x55, 0x48, 0x89, 0xe5]),
        ];
        let plan = plan_assembly_patch(
            &rows,
            &[0xe9, 0x11, 0x22, 0x33, 0x44],
            ExecutableArch::X86_64,
        )
        .unwrap();
        assert_eq!(plan.patch_len, 7);
        assert_eq!(plan.covered_rows, 2);
        assert_eq!(plan.trailing_nop_len, 2);
        assert_eq!(
            plan.patch_bytes,
            vec![0xe9, 0x11, 0x22, 0x33, 0x44, 0x90, 0x90]
        );
    }

    #[test]
    fn aarch64_patch_uses_four_byte_nops() {
        let rows = vec![row(0x100, &[0xfd, 0x7b, 0xbf, 0xa9])];
        let plan =
            plan_assembly_patch(&rows, &[0xc0, 0x03, 0x5f, 0xd6], ExecutableArch::AArch64).unwrap();
        assert_eq!(plan.patch_bytes, vec![0xc0, 0x03, 0x5f, 0xd6]);
    }
}
