use crate::app::{App, DisasmEdit, EditOp, ReplacementChange};
use crate::disasm::assembler::resolve_assembler_backend;
use crate::disasm::{plan_assembly_patch, DisasmRow, DisasmRowKind};
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

use super::text_cursor::{
    backspace_char_before_cursor, delete_char_at_cursor, insert_char_at_cursor, move_cursor_end,
    move_cursor_home, move_cursor_left, move_cursor_right,
};

impl App {
    pub(crate) fn begin_disasm_edit(&mut self) -> HxResult<()> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        let Some(row) = self.current_disassembly_row()? else {
            return Ok(());
        };
        if row.kind != DisasmRowKind::Instruction {
            return Err(HxError::AssemblyError(
                "assembly patch only applies to instruction rows".to_owned(),
            ));
        }
        let Some(state) = self.current_disassembly_state() else {
            return Ok(());
        };
        let _ = resolve_assembler_backend(&state.info, None)?;
        self.disasm_edit = Some(DisasmEdit {
            row_offset: row.offset,
            buffer: row.assembly_text.clone(),
            cursor_pos: row.assembly_text.len(),
        });
        self.mode = Mode::DisasmEdit;
        Ok(())
    }

    pub(crate) fn cancel_disasm_edit(&mut self) {
        self.disasm_edit = None;
        if matches!(self.mode, Mode::DisasmEdit) {
            self.mode = Mode::Normal;
        }
    }

    pub(crate) fn submit_disasm_edit(&mut self) -> HxResult<()> {
        let Some(edit) = self.disasm_edit.take() else {
            return Ok(());
        };
        self.mode = Mode::Normal;

        let Some(state) = self.current_disassembly_state() else {
            return Ok(());
        };
        let row = self
            .collect_disassembly_rows(&state, edit.row_offset, 1)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                HxError::AssemblyError("instruction row disappeared before submit".to_owned())
            })?;
        if row.offset != edit.row_offset || row.kind != DisasmRowKind::Instruction {
            return Err(HxError::AssemblyError(
                "instruction row changed before submit".to_owned(),
            ));
        }

        let backend = resolve_assembler_backend(&state.info, None)?;
        let address = row.virtual_address.unwrap_or(row.offset);
        let assembled = backend.assemble_one(
            state.info.arch,
            state.info.bitness,
            state.info.endian,
            address,
            &edit.buffer,
        )?;
        let rows = self.collect_patch_rows(&state, row.offset, assembled.len())?;
        let plan = plan_assembly_patch(&rows, &assembled, state.info.arch)?;
        let ops = self.apply_disassembly_patch(row.offset, &plan.patch_bytes)?;
        let changed = !ops.is_empty();
        if changed {
            self.push_undo_step(ops, row.offset, Mode::Normal, row.offset, Mode::Normal);
            self.invalidate_disassembly_cache();
            self.refresh_inspector();
        }

        self.set_disassembly_patch_status(&row, &plan, changed);
        Ok(())
    }

    pub(crate) fn insert_disasm_char(&mut self, c: char) {
        if let Some(edit) = self.disasm_edit.as_mut() {
            insert_char_at_cursor(&mut edit.buffer, &mut edit.cursor_pos, c);
        }
    }

    pub(crate) fn backspace_disasm_char(&mut self) {
        if let Some(edit) = self.disasm_edit.as_mut() {
            backspace_char_before_cursor(&mut edit.buffer, &mut edit.cursor_pos);
        }
    }

    pub(crate) fn move_disasm_cursor(&mut self, left: bool) {
        if let Some(edit) = self.disasm_edit.as_mut() {
            if left {
                move_cursor_left(&edit.buffer, &mut edit.cursor_pos);
            } else {
                move_cursor_right(&edit.buffer, &mut edit.cursor_pos);
            }
        }
    }

    pub(crate) fn set_disasm_cursor(&mut self, home: bool) {
        if let Some(edit) = self.disasm_edit.as_mut() {
            if home {
                move_cursor_home(&mut edit.cursor_pos);
            } else {
                move_cursor_end(&edit.buffer, &mut edit.cursor_pos);
            }
        }
    }

    pub(crate) fn delete_disasm_char(&mut self) {
        if let Some(edit) = self.disasm_edit.as_mut() {
            delete_char_at_cursor(&mut edit.buffer, edit.cursor_pos);
        }
    }

    pub(crate) fn disasm_edit(&self) -> Option<&DisasmEdit> {
        self.disasm_edit.as_ref()
    }

    fn current_disassembly_row(&mut self) -> HxResult<Option<DisasmRow>> {
        let Some(state) = self.current_disassembly_state() else {
            return Ok(None);
        };
        Ok(self
            .collect_disassembly_rows(&state, self.cursor_anchor_offset(), 1)?
            .into_iter()
            .next())
    }

    fn collect_patch_rows(
        &mut self,
        state: &crate::disasm::DisassemblyState,
        offset: u64,
        assembled_len: usize,
    ) -> HxResult<Vec<DisasmRow>> {
        let target_len = assembled_len.max(1);
        let mut rows = Vec::new();
        let mut covered = 0usize;
        let mut start = offset;

        while covered < target_len && rows.len() < 256 {
            let chunk = self.collect_disassembly_rows(state, start, 64)?;
            if chunk.is_empty() {
                break;
            }
            for row in chunk {
                covered = covered.saturating_add(row.len());
                start = row.offset.saturating_add(row.len() as u64);
                rows.push(row);
                if covered >= target_len || start >= self.document.len() {
                    break;
                }
            }
        }

        Ok(rows)
    }

    fn apply_disassembly_patch(&mut self, offset: u64, bytes: &[u8]) -> HxResult<Vec<EditOp>> {
        let ids = self.document.cell_ids_range(offset, bytes.len() as u64);
        if ids.len() != bytes.len() {
            return Err(HxError::AssemblyError(
                "assembled instruction exceeds remaining display bytes".to_owned(),
            ));
        }

        let mut changes = Vec::new();
        for (id, &byte) in ids.into_iter().zip(bytes.iter()) {
            if self.document.is_tombstone(id) {
                return Err(HxError::AssemblyError(
                    "cannot patch over deleted display slots".to_owned(),
                ));
            }
            let before = self.document.replacement_state(id);
            self.document.replace_display_byte_by_id(id, byte)?;
            let after = self.document.replacement_state(id);
            if after != before {
                changes.push(ReplacementChange { id, before, after });
            }
        }

        if changes.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![EditOp::ReplaceBytes { changes }])
        }
    }

    fn set_disassembly_patch_status(
        &mut self,
        row: &DisasmRow,
        plan: &crate::disasm::AssemblyPatchPlan,
        changed: bool,
    ) {
        let label = format!("patched {}", row.label());
        if !changed {
            self.set_info_status(format!("{label}; bytes unchanged"));
            return;
        }

        if plan.patch_len > plan.original_len {
            self.set_warning_status(format!(
                "{label}; {} bytes > old {}, covered {} rows, trailing nop {}",
                plan.patch_bytes.len(),
                plan.original_len,
                plan.covered_rows,
                plan.trailing_nop_len
            ));
        } else if plan.trailing_nop_len > 0 {
            self.set_info_status(format!(
                "{label}; {}→{} bytes with {} trailing nop",
                plan.patch_bytes.len() - plan.trailing_nop_len,
                plan.patch_len,
                plan.trailing_nop_len
            ));
        } else {
            self.set_info_status(format!("{label}; {} bytes", plan.patch_len));
        }
    }
}
