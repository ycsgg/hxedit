use crate::app::{App, UndoStep};
use crate::error::HxResult;
use crate::exec::{apply_edit_op, edit_op_has_effect, undo_edit_op, EditOp};
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
                undo_edit_op(&mut self.document, op)?;
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
                apply_edit_op(&mut self.document, op)?;
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
