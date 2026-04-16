use crate::app::{App, EditOp, UndoStep};
use crate::error::HxResult;
use crate::mode::Mode;

impl App {
    /// Undo `steps` edit actions by popping from the undo stack and replaying
    /// each operation in reverse.
    ///
    /// Each [`UndoStep`] records the cursor position and mode *before* the
    /// edit, plus a list of [`EditOp`]s that describe what changed.  Undoing
    /// replays the inverse of each op:
    ///
    /// - `Insert` → real-delete the inserted bytes
    /// - `RealDelete` → re-insert the removed cells
    /// - `TombstoneDelete` → clear the tombstones
    /// - `ReplaceBytes` → restore each cell's previous replacement state
    ///
    /// When `restore_mode` is true the mode is set back to what it was before
    /// the edit (used by Ctrl-Z in edit/insert modes).  When false the mode
    /// is forced to Normal (used by `:u`).
    pub(crate) fn undo(&mut self, steps: usize, restore_mode: bool) -> HxResult<()> {
        let mut undone = 0;

        for _ in 0..steps {
            let Some(step) = self.undo_stack.pop() else {
                break;
            };

            for op in step.ops.iter().rev() {
                match op {
                    EditOp::Insert { offset, len } => {
                        self.document.delete_range_real(*offset, *len)?;
                    }
                    EditOp::RealDelete { offset, cells } => {
                        self.document.restore_real_delete(*offset, cells)?;
                    }
                    EditOp::TombstoneDelete { ids } => {
                        self.document.clear_tombstones(ids);
                    }
                    EditOp::ReplaceBytes { changes } => {
                        for change in changes {
                            self.document
                                .restore_replacement(change.id, change.previous)?;
                        }
                    }
                }
            }

            self.cursor = self.clamp_cursor_for_mode(step.cursor_before, step.mode_before);
            if restore_mode {
                self.mode = step.mode_before;
            }
            undone += 1;
        }

        if !restore_mode {
            self.mode = Mode::Normal;
            self.cursor = self.clamp_cursor_for_mode(self.cursor, Mode::Normal);
        }

        self.refresh_inspector();

        if undone == 0 {
            self.status_message = "nothing to undo".to_owned();
        } else if undone == 1 {
            self.status_message = "undid 1 action".to_owned();
        } else {
            self.status_message = format!("undid {undone} actions");
        }

        Ok(())
    }

    /// Push a new undo step onto the stack.
    pub(crate) fn push_undo_step(
        &mut self,
        ops: Vec<EditOp>,
        cursor_before: u64,
        mode_before: Mode,
    ) {
        if ops.is_empty() {
            return;
        }
        self.undo_stack.push(UndoStep {
            cursor_before,
            mode_before,
            ops,
        });
    }
}
