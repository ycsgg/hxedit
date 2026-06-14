use crate::app::{App, BulkReplacement, EditOp, UndoStep};
use crate::error::HxResult;
use crate::mode::Mode;

impl App {
    /// Undo `steps` edit actions by popping from the undo stack and replaying
    /// each operation in reverse.
    pub(crate) fn undo(&mut self, steps: usize, restore_mode: bool) -> HxResult<()> {
        let mut undone = 0;

        for _ in 0..steps {
            let Some(step) = self.undo_stack.pop() else {
                break;
            };

            for op in step.ops.iter().rev() {
                self.undo_edit_op(op)?;
            }

            self.cursor = self.clamp_cursor_for_mode(step.cursor_before, step.mode_before);
            if restore_mode {
                self.mode = step.mode_before;
            }
            self.redo_stack.push(step);
            undone += 1;
        }

        if !restore_mode {
            self.mode = Mode::Normal;
            self.cursor = self.clamp_cursor_for_mode(self.cursor, Mode::Normal);
        }

        if undone > 0 {
            self.mark_document_changed();
            #[cfg(feature = "sagitta-analysis")]
            self.mark_sagitta_edit_ops(
                &self
                    .redo_stack
                    .iter()
                    .rev()
                    .take(undone)
                    .flat_map(|step| step.ops.iter().cloned())
                    .collect::<Vec<_>>(),
            );
            self.invalidate_disassembly_cache();
        }
        self.refresh_inspector();
        self.set_undo_status(undone);
        Ok(())
    }

    pub(crate) fn redo(&mut self, steps: usize, restore_mode: bool) -> HxResult<()> {
        let mut redone = 0;

        for _ in 0..steps {
            let Some(step) = self.redo_stack.pop() else {
                break;
            };

            for op in &step.ops {
                self.apply_edit_op(op)?;
            }

            self.cursor = self.clamp_cursor_for_mode(step.cursor_after, step.mode_after);
            if restore_mode {
                self.mode = step.mode_after;
            }
            self.undo_stack.push(step);
            redone += 1;
        }

        if !restore_mode {
            self.mode = Mode::Normal;
            self.cursor = self.clamp_cursor_for_mode(self.cursor, Mode::Normal);
        }

        if redone > 0 {
            self.mark_document_changed();
            #[cfg(feature = "sagitta-analysis")]
            self.mark_sagitta_edit_ops(
                &self
                    .undo_stack
                    .iter()
                    .rev()
                    .take(redone)
                    .flat_map(|step| step.ops.iter().cloned())
                    .collect::<Vec<_>>(),
            );
            self.invalidate_disassembly_cache();
        }
        self.refresh_inspector();

        if redone == 0 {
            self.set_info_status("nothing to redo");
        } else if redone == 1 {
            self.set_info_status("redid 1 action");
        } else {
            self.set_info_status(format!("redid {redone} actions"));
        }

        Ok(())
    }

    fn apply_edit_op(&mut self, op: &EditOp) -> HxResult<()> {
        match op {
            EditOp::Insert { offset, cells } => {
                self.document.restore_real_delete(*offset, cells)?
            }
            EditOp::RealDelete { offset, cells } => {
                let removed = self
                    .document
                    .delete_range_real(*offset, cells.len() as u64)?;
                debug_assert_eq!(removed, *cells);
            }
            EditOp::TombstoneDelete { ids } => self.document.mark_tombstones(ids)?,
            EditOp::ReplaceBytes { changes } => {
                for change in changes {
                    self.document.restore_replacement(change.id, change.after)?;
                }
            }
            EditOp::ReplaceBulk {
                offset, len, after, ..
            } => self.apply_bulk_replacement(*offset, *len, after)?,
            EditOp::ReplacePatch {
                offset, len, after, ..
            } => self
                .document
                .restore_replacement_patch_in_display_range(*offset, *len, after)?,
        }
        Ok(())
    }

    fn undo_edit_op(&mut self, op: &EditOp) -> HxResult<()> {
        match op {
            EditOp::Insert { offset, cells } => {
                let removed = self
                    .document
                    .delete_range_real(*offset, cells.len() as u64)?;
                debug_assert_eq!(removed, *cells);
            }
            EditOp::RealDelete { offset, cells } => {
                self.document.restore_real_delete(*offset, cells)?
            }
            EditOp::TombstoneDelete { ids } => self.document.clear_tombstones(ids),
            EditOp::ReplaceBytes { changes } => {
                for change in changes {
                    self.document
                        .restore_replacement(change.id, change.before)?;
                }
            }
            EditOp::ReplaceBulk {
                offset,
                len,
                before,
                ..
            } => self.apply_bulk_replacement(*offset, *len, before)?,
            EditOp::ReplacePatch {
                offset,
                len,
                before,
                ..
            } => self
                .document
                .restore_replacement_patch_in_display_range(*offset, *len, before)?,
        }
        Ok(())
    }

    fn apply_bulk_replacement(
        &mut self,
        offset: u64,
        len: u64,
        replacement: &BulkReplacement,
    ) -> HxResult<()> {
        let end = offset
            .checked_add(len)
            .ok_or(crate::error::HxError::OffsetOutOfRange)?;
        if end > self.document.len() {
            return Err(crate::error::HxError::OffsetOutOfRange);
        }

        match replacement {
            BulkReplacement::Clear => self
                .document
                .clear_replacements_in_display_range(offset, len),
            BulkReplacement::Pattern(pattern) => {
                if len == 0 {
                    return Ok(());
                }
                if pattern.is_empty() {
                    return Err(crate::error::HxError::CommandError(
                        "bulk replacement pattern must not be empty".to_owned(),
                    ));
                }
                self.document
                    .overwrite_run_pattern_overlay(offset, len, pattern)?;
                Ok(())
            }
            BulkReplacement::Bytes(bytes) => {
                if len == 0 {
                    return Ok(());
                }
                if bytes.len() as u64 != len {
                    return Err(crate::error::HxError::CommandError(
                        "bulk replacement byte run length mismatch".to_owned(),
                    ));
                }
                self.document
                    .overwrite_run_bytes_overlay(offset, std::sync::Arc::clone(bytes))?;
                Ok(())
            }
            BulkReplacement::Xor { key } => {
                if len == 0 {
                    return Ok(());
                }
                self.document
                    .xor_visible_range_overlay(offset, offset + len - 1, *key)?;
                Ok(())
            }
        }
    }

    fn set_undo_status(&mut self, undone: usize) {
        if undone == 0 {
            self.set_info_status("nothing to undo");
        } else if undone == 1 {
            self.set_info_status("undid 1 action");
        } else {
            self.set_info_status(format!("undid {undone} actions"));
        }
    }

    /// Push a new undo step onto the stack.
    pub(crate) fn push_undo_step(
        &mut self,
        ops: Vec<EditOp>,
        cursor_before: u64,
        mode_before: Mode,
        cursor_after: u64,
        mode_after: Mode,
    ) {
        if ops.is_empty() {
            return;
        }
        if !ops.iter().any(edit_op_has_effect) {
            return;
        }
        #[cfg(feature = "sagitta-analysis")]
        self.mark_sagitta_edit_ops(&ops);
        self.undo_stack.push(UndoStep {
            cursor_before,
            mode_before,
            cursor_after,
            mode_after,
            ops,
        });
        self.redo_stack.clear();
        self.mark_document_changed();
    }
}

fn edit_op_has_effect(op: &EditOp) -> bool {
    match op {
        EditOp::Insert { cells, .. } | EditOp::RealDelete { cells, .. } => !cells.is_empty(),
        EditOp::TombstoneDelete { ids } => !ids.is_empty(),
        EditOp::ReplaceBytes { changes } => {
            changes.iter().any(|change| change.before != change.after)
        }
        EditOp::ReplaceBulk {
            len, before, after, ..
        } => *len > 0 && before != after,
        EditOp::ReplacePatch {
            len, before, after, ..
        } => *len > 0 && before != after,
    }
}
