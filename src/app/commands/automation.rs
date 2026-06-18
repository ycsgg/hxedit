#[cfg(feature = "scripting")]
use std::path::Path;
use std::path::PathBuf;

use super::*;
#[cfg(feature = "scripting")]
use crate::exec::ExecStep;
use crate::exec::{ExecRange, ExecSelection, ExecState, RangeSpace};

impl App {
    pub(super) fn execute_source_command(&mut self, path: PathBuf) -> HxResult<()> {
        let program = crate::automation::MacroProgram::load(&path)?;
        let initial_selection = self.active_exec_selection();
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
            self.apply_exec_state_after_automation(state);
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

    #[cfg(feature = "scripting")]
    pub(super) fn execute_script_command(&mut self, path: PathBuf) -> HxResult<()> {
        let source = std::fs::read_to_string(&path)?;
        let state = ExecState::new(self.cursor, self.active_exec_selection());
        let placeholder = crate::core::document::Document::from_memory_bytes(
            PathBuf::from("<script-placeholder>"),
            Vec::new(),
            &self.config,
        );
        let document = std::mem::replace(&mut self.document, placeholder);
        let result = crate::scripting::run_script_source(&path, &source, document, state);

        match result {
            Ok(result) => {
                self.document = result.document;
                self.apply_script_report(&path, result.state, result.report, None)
            }
            Err(failure) => {
                let failure = *failure;
                self.document = failure.document;
                self.apply_script_report(&path, failure.state, failure.report, Some(failure.error))
            }
        }
    }

    #[cfg(not(feature = "scripting"))]
    pub(super) fn execute_script_command(&mut self, path: PathBuf) -> HxResult<()> {
        Err(HxError::CommandError(format!(
            "script {} requires a build with the scripting feature",
            path.display()
        )))
    }

    #[cfg(feature = "scripting")]
    fn apply_script_report(
        &mut self,
        path: &Path,
        state: ExecState,
        report: crate::scripting::ScriptReport,
        error: Option<HxError>,
    ) -> HxResult<()> {
        if report.saved {
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.mark_document_changed();
        }

        let mode_for_undo = self.mode;
        self.push_script_undo_step(&report.undo_steps, mode_for_undo);

        if error.is_some() {
            self.selection_anchor = None;
            self.cursor = self.clamp_cursor_for_mode(state.cursor, self.mode);
        } else {
            self.apply_exec_state_after_automation(state);
        }

        if report.saved || !report.undo_steps.is_empty() {
            self.invalidate_disassembly_cache();
            self.refresh_inspector();
        }

        if let Some(error) = error {
            return Err(error);
        }

        let summary = report
            .summaries
            .last()
            .map(String::as_str)
            .unwrap_or("no calls");
        self.set_info_status(format!(
            "script {}: ran {} calls; {summary}",
            path.display(),
            report.exec_calls
        ));
        Ok(())
    }

    fn active_exec_selection(&self) -> Option<ExecSelection> {
        self.active_selection_range()
            .map(|(start, end)| ExecSelection {
                range: ExecRange::display(start, end - start + 1),
            })
    }

    #[cfg(feature = "scripting")]
    fn push_script_undo_step(&mut self, steps: &[ExecStep], mode_for_undo: Mode) {
        let Some(first) = steps.first() else {
            return;
        };
        let last = steps.last().unwrap_or(first);
        let ops = steps
            .iter()
            .flat_map(|step| step.ops.iter().cloned())
            .collect::<Vec<_>>();
        self.push_undo_step(
            ops,
            first.cursor_before,
            mode_for_undo,
            last.cursor_after,
            mode_for_undo,
        );
    }

    fn apply_exec_state_after_automation(&mut self, state: ExecState) {
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
