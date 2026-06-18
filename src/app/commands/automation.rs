use std::path::PathBuf;

use super::*;
use crate::exec::{ExecRange, ExecSelection, ExecState, RangeSpace};

impl App {
    pub(super) fn execute_source_command(&mut self, path: PathBuf) -> HxResult<()> {
        let program = crate::automation::MacroProgram::load(&path)?;
        let initial_selection = self
            .active_selection_range()
            .map(|(start, end)| ExecSelection {
                range: ExecRange::display(start, end - start + 1),
            });
        let mut state = ExecState::new(self.cursor, initial_selection);
        let report = program.execute(&mut self.document, &mut state)?;

        if report.saved {
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.mark_document_changed();
        }

        let mode_for_undo = self.mode;
        for step in &report.undo_steps {
            self.push_undo_step(
                step.ops.clone(),
                step.cursor_before,
                mode_for_undo,
                step.cursor_after,
                mode_for_undo,
            );
        }

        if report.error.is_some() {
            self.selection_anchor = None;
            self.cursor = self.clamp_cursor_for_mode(state.cursor, self.mode);
        } else {
            self.apply_exec_state_after_source(state);
        }

        if report.saved || !report.undo_steps.is_empty() {
            self.invalidate_disassembly_cache();
            self.refresh_inspector();
        }

        let path_label = path.display();
        let summary = report
            .outcomes
            .last()
            .map(|outcome| outcome.summary.as_str())
            .unwrap_or("no steps");

        if let Some(error) = report.error {
            return Err(HxError::CommandError(format!(
                "source {path_label}: stopped after {} steps: {error}",
                report.steps_completed
            )));
        }

        self.set_info_status(format!(
            "source {path_label}: ran {} steps; {summary}",
            report.steps_completed
        ));

        Ok(())
    }

    fn apply_exec_state_after_source(&mut self, state: ExecState) {
        match state.selection {
            Some(selection) if selection.range.space == RangeSpace::Display => {
                let start = selection.range.start;
                let end = selection
                    .range
                    .end_exclusive()
                    .ok()
                    .and_then(|end| end.checked_sub(1));
                if let Some(end) = end {
                    self.cursor = self.clamp_cursor_for_mode(start, Mode::Visual);
                    self.selection_anchor = Some(self.clamp_offset(end));
                    self.mode = Mode::Visual;
                    return;
                }
            }
            Some(_) | None => {}
        }

        self.selection_anchor = None;
        self.cursor = self.clamp_cursor_for_mode(state.cursor, self.mode);
    }
}
