use crate::app::{App, EditOp, UndoStep};
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
        }
        Ok(())
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
        self.undo_stack.push(UndoStep {
            cursor_before,
            mode_before,
            cursor_after,
            mode_after,
            ops,
        });
        self.redo_stack.clear();
    }
}
