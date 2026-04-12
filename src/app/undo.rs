use crate::app::{App, UndoEntry, UndoStep};
use crate::error::HxResult;
use crate::mode::Mode;

impl App {
    pub(crate) fn undo(&mut self, steps: usize, restore_mode: bool) -> HxResult<()> {
        let mut undone = 0;

        for _ in 0..steps {
            let Some(step) = self.undo_stack.pop() else {
                break;
            };
            for entry in step.entries.iter().rev() {
                self.document
                    .restore_patch_state(entry.offset, entry.previous_patch)?;
            }
            if let Some(entry) = step.entries.first() {
                self.cursor = self.clamp_offset(entry.cursor_before);
                if restore_mode {
                    self.mode = entry.mode_before;
                }
            }
            undone += 1;
        }

        if !restore_mode {
            self.mode = Mode::Normal;
        }

        if undone == 0 {
            self.status_message = "nothing to undo".to_owned();
        } else if undone == 1 {
            self.status_message = "undid 1 action".to_owned();
        } else {
            self.status_message = format!("undid {undone} actions");
        }

        Ok(())
    }

    pub(crate) fn push_undo_if_changed(
        &mut self,
        offset: u64,
        previous_patch: crate::core::patch::PatchState,
        cursor_before: u64,
        mode_before: Mode,
    ) {
        if self.document.patch_state_at(offset) != previous_patch {
            self.push_undo_step(vec![UndoEntry {
                offset,
                previous_patch,
                cursor_before,
                mode_before,
            }]);
        }
    }

    pub(crate) fn push_undo_step(&mut self, entries: Vec<UndoEntry>) {
        if entries.is_empty() {
            return;
        }
        self.undo_stack.push(UndoStep { entries });
    }
}
