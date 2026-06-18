use std::path::PathBuf;

use crate::commands::types::HashAlgorithm;
use crate::core::document::Document;
use crate::error::{HxError, HxResult};

use super::{
    edit_op_has_effect, export_binary_range, fill_overwrite, hash_display_range, insert_bytes,
    overwrite_bytes, real_delete_range, replace_range, tombstone_delete_range, undo_edit_op,
    xor_in_place, EditOp, ExecArtifact, ExecOutcome, ExecRange, ExecSelection, RangeSpace,
    ReplaceResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecState {
    pub cursor: u64,
    pub selection: Option<ExecSelection>,
}

impl ExecState {
    pub fn new(cursor: u64, selection: Option<ExecSelection>) -> Self {
        Self { cursor, selection }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecCommand {
    Goto {
        target: ExecOffset,
    },
    Select {
        range: ExecRange,
    },
    ClearSelection,
    Read {
        scope: ExecScope,
    },
    Hash {
        algorithm: HashAlgorithm,
        scope: ExecScope,
    },
    Search {
        pattern: Vec<u8>,
        direction: ExecSearchDirection,
        select: SearchSelect,
    },
    Overwrite {
        offset: ExecOffset,
        bytes: Vec<u8>,
    },
    Insert {
        offset: ExecOffset,
        bytes: Vec<u8>,
    },
    Delete {
        scope: ExecScope,
        kind: DeleteKind,
    },
    Fill {
        offset: ExecOffset,
        pattern: Vec<u8>,
        len: u64,
    },
    XorInPlace {
        scope: ExecScope,
        key: u8,
    },
    Replace {
        scope: ExecScope,
        needle: Vec<u8>,
        replacement: Vec<u8>,
        allow_resize: bool,
        force: bool,
    },
    ExportBinary {
        scope: ExecScope,
        path: PathBuf,
    },
    Save {
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecOffset {
    Absolute(u64),
    Cursor(i64),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecScope {
    Selection,
    Range(ExecRange),
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecSearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSelect {
    None,
    Match,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteKind {
    Tombstone,
    Real,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecUndoPolicy {
    Group,
    PerStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecErrorPolicy {
    Stop,
    Rollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecBatchOptions {
    pub undo: ExecUndoPolicy,
    pub on_error: ExecErrorPolicy,
}

impl Default for ExecBatchOptions {
    fn default() -> Self {
        Self {
            undo: ExecUndoPolicy::Group,
            on_error: ExecErrorPolicy::Stop,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExecStep {
    pub cursor_before: u64,
    #[allow(dead_code)]
    pub selection_before: Option<ExecSelection>,
    pub cursor_after: u64,
    #[allow(dead_code)]
    pub selection_after: Option<ExecSelection>,
    pub ops: Vec<EditOp>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ExecBatchReport {
    pub outcomes: Vec<ExecOutcome>,
    pub undo_steps: Vec<ExecStep>,
    pub steps_completed: usize,
    pub saved: bool,
    pub error: Option<String>,
}

pub(crate) fn execute_batch(
    document: &mut Document,
    state: &mut ExecState,
    commands: &[ExecCommand],
    options: ExecBatchOptions,
) -> HxResult<ExecBatchReport> {
    if options.on_error == ExecErrorPolicy::Rollback
        && commands.iter().any(|command| {
            matches!(
                command,
                ExecCommand::Save { .. } | ExecCommand::ExportBinary { .. }
            )
        })
    {
        return Err(HxError::CommandError(
            "rollback macros cannot contain save or export-binary steps".to_owned(),
        ));
    }

    let initial_state = *state;
    let mut report = ExecBatchReport::default();

    let mut group_start = initial_state;
    let mut group_ops = Vec::new();
    let mut applied_ops = Vec::new();

    for command in commands {
        let step_before = *state;
        let result = execute_one(document, state, command);
        let step_result = match result {
            Ok(step_result) => step_result,
            Err(err) => {
                if options.on_error == ExecErrorPolicy::Rollback {
                    for op in applied_ops.iter().rev() {
                        undo_edit_op(document, op)?;
                    }
                    *state = initial_state;
                    group_ops.clear();
                    applied_ops.clear();
                    report.undo_steps.clear();
                    report.outcomes.clear();
                }
                report.error = Some(err.to_string());
                break;
            }
        };

        if step_result.saved {
            group_ops.clear();
            applied_ops.clear();
            report.undo_steps.clear();
            report.saved = true;
        }

        let effective_ops = step_result
            .ops
            .into_iter()
            .filter(edit_op_has_effect)
            .collect::<Vec<_>>();
        if !effective_ops.is_empty() {
            applied_ops.extend(effective_ops.iter().cloned());
            match options.undo {
                ExecUndoPolicy::Group => {
                    if group_ops.is_empty() {
                        group_start = step_before;
                    }
                    group_ops.extend(effective_ops);
                }
                ExecUndoPolicy::PerStep => report.undo_steps.push(ExecStep {
                    cursor_before: step_before.cursor,
                    selection_before: step_before.selection,
                    cursor_after: state.cursor,
                    selection_after: state.selection,
                    ops: effective_ops,
                }),
            }
        }

        report.outcomes.push(step_result.outcome);
        report.steps_completed += 1;
    }

    if options.undo == ExecUndoPolicy::Group && !group_ops.is_empty() {
        report.undo_steps.push(ExecStep {
            cursor_before: group_start.cursor,
            selection_before: group_start.selection,
            cursor_after: state.cursor,
            selection_after: state.selection,
            ops: group_ops,
        });
    }

    Ok(report)
}

struct StepResult {
    outcome: ExecOutcome,
    ops: Vec<EditOp>,
    saved: bool,
}

fn execute_one(
    document: &mut Document,
    state: &mut ExecState,
    command: &ExecCommand,
) -> HxResult<StepResult> {
    match command {
        ExecCommand::Goto { target } => {
            let target = resolve_offset(document, state, *target)?;
            state.cursor = document.goto(target)?;
            let mut outcome = ExecOutcome::new(
                format!("moved to display 0x{:x}", state.cursor),
                document.is_dirty(),
            );
            outcome.cursor = Some(state.cursor);
            Ok(step(outcome, Vec::new()))
        }
        ExecCommand::Select { range } => {
            range.display_bounds(document)?;
            state.selection = Some(ExecSelection { range: *range });
            let mut outcome = ExecOutcome::new("selected range", document.is_dirty());
            outcome.selection = Some(*range);
            Ok(step(outcome, Vec::new()))
        }
        ExecCommand::ClearSelection => {
            state.selection = None;
            Ok(step(
                ExecOutcome::new("cleared selection", document.is_dirty()),
                Vec::new(),
            ))
        }
        ExecCommand::Read { scope } => {
            if matches!(scope, ExecScope::All) {
                return Err(HxError::CommandError(
                    "macro read does not support scope = \"all\"; use an explicit range".to_owned(),
                ));
            }
            let Some((start, end)) = resolve_scope(document, state, *scope)? else {
                let mut outcome = ExecOutcome::new("read 0 bytes", document.is_dirty());
                outcome.bytes_read = Some(0);
                outcome.artifacts.push(ExecArtifact::Bytes(Vec::new()));
                return Ok(step(outcome, Vec::new()));
            };
            let bytes = document.logical_bytes(start, end)?;
            let mut outcome = ExecOutcome::new(
                format!("read {} logical bytes", bytes.len()),
                document.is_dirty(),
            );
            outcome.bytes_read = Some(bytes.len() as u64);
            outcome.artifacts.push(ExecArtifact::Bytes(bytes));
            Ok(step(outcome, Vec::new()))
        }
        ExecCommand::Hash { algorithm, scope } => {
            let Some((start, end)) = resolve_scope(document, state, *scope)? else {
                let mut outcome = ExecOutcome::new(
                    format!("{}: no data to hash", algorithm.label()),
                    document.is_dirty(),
                );
                outcome.bytes_read = Some(0);
                return Ok(step(outcome, Vec::new()));
            };
            let hash = hash_display_range(document, *algorithm, start, end)?;
            if hash.bytes_hashed == 0 {
                let mut outcome = ExecOutcome::new(
                    format!("{}: no data to hash", algorithm.label()),
                    document.is_dirty(),
                );
                outcome.bytes_read = Some(0);
                return Ok(step(outcome, Vec::new()));
            }
            let mut outcome = ExecOutcome::new(
                format!(
                    "{} [0x{start:x}-0x{end:x}]: {} ({} bytes)",
                    algorithm.label(),
                    hash.hex,
                    hash.bytes_hashed
                ),
                document.is_dirty(),
            );
            outcome.bytes_read = Some(hash.bytes_hashed);
            outcome.artifacts.push(ExecArtifact::Text(hash.hex));
            Ok(step(outcome, Vec::new()))
        }
        ExecCommand::Search {
            pattern,
            direction,
            select,
        } => {
            if pattern.is_empty() {
                return Err(HxError::EmptySearch);
            }
            let found = match direction {
                ExecSearchDirection::Forward => document.search_forward(state.cursor, pattern)?,
                ExecSearchDirection::Backward => document.search_backward(state.cursor, pattern)?,
            };
            let Some(found) = found else {
                return Ok(step(
                    ExecOutcome::new("search: not found", document.is_dirty()),
                    Vec::new(),
                ));
            };
            state.cursor = document.goto(found)?;
            if *select == SearchSelect::Match {
                let end = display_end_for_match(document, found, pattern.len() as u64)?;
                state.selection = Some(ExecSelection {
                    range: ExecRange::display(found, end - found + 1),
                });
            }
            let mut outcome =
                ExecOutcome::new(format!("search: display 0x{found:x}"), document.is_dirty());
            outcome.cursor = Some(found);
            outcome.selection = state.selection.map(|selection| selection.range);
            Ok(step(outcome, Vec::new()))
        }
        ExecCommand::Overwrite { offset, bytes } => {
            let offset = resolve_offset(document, state, *offset)?;
            let result = overwrite_bytes(document, offset, bytes)?;
            if result.written > 0 {
                state.cursor = clamp_cursor(document, offset + result.written - 1);
            }
            let mut outcome = ExecOutcome::new(
                format!("overwrote {} bytes", result.written),
                document.is_dirty(),
            );
            outcome.cursor = Some(state.cursor);
            outcome.bytes_changed = Some(result.written);
            Ok(step(outcome, result.ops))
        }
        ExecCommand::Insert { offset, bytes } => {
            let offset = resolve_offset(document, state, *offset)?;
            let result = insert_bytes(document, offset, bytes)?;
            if result.inserted > 0 {
                state.cursor = clamp_cursor(document, offset + result.inserted as u64 - 1);
            }
            state.selection = None;
            let mut outcome = ExecOutcome::new(
                format!("inserted {} bytes", result.inserted),
                document.is_dirty(),
            );
            outcome.cursor = Some(state.cursor);
            outcome.bytes_changed = Some(result.inserted as u64);
            Ok(step(outcome, result.ops))
        }
        ExecCommand::Delete { scope, kind } => {
            let Some((start, end)) = resolve_scope(document, state, *scope)? else {
                return Ok(step(
                    ExecOutcome::new("deleted 0 bytes", document.is_dirty()),
                    Vec::new(),
                ));
            };
            let result = match kind {
                DeleteKind::Tombstone => tombstone_delete_range(document, start, end)?,
                DeleteKind::Real => real_delete_range(document, start, end - start + 1)?,
            };
            state.cursor = clamp_cursor(document, start);
            match kind {
                DeleteKind::Tombstone => clear_logical_selection_after_tombstone(state),
                DeleteKind::Real => state.selection = None,
            }
            let mut outcome = ExecOutcome::new(
                format!("deleted {} bytes", result.deleted),
                document.is_dirty(),
            );
            outcome.cursor = Some(state.cursor);
            outcome.bytes_changed = Some(result.deleted);
            Ok(step(outcome, result.ops))
        }
        ExecCommand::Fill {
            offset,
            pattern,
            len,
        } => {
            let offset = resolve_offset(document, state, *offset)?;
            let result = fill_overwrite(document, offset, pattern, *len)?;
            if result.written > 0 {
                state.cursor = clamp_cursor(document, offset + result.written - 1);
            }
            let mut outcome = ExecOutcome::new(
                format!("filled {} bytes", result.written),
                document.is_dirty(),
            );
            outcome.cursor = Some(state.cursor);
            outcome.bytes_changed = Some(result.changed);
            if result.written < *len {
                outcome.warnings.push("truncated at EOF".to_owned());
            }
            Ok(step(outcome, result.ops))
        }
        ExecCommand::XorInPlace { scope, key } => {
            let Some((start, end)) = resolve_scope(document, state, *scope)? else {
                return Ok(step(
                    ExecOutcome::new("xor!: no logical bytes in scope", document.is_dirty()),
                    Vec::new(),
                ));
            };
            let result = xor_in_place(document, start, end, *key)?;
            state.cursor = clamp_cursor(document, start);
            let mut outcome = ExecOutcome::new(
                format!(
                    "xor! 0x{key:02x}: replaced {} logical bytes",
                    result.visited
                ),
                document.is_dirty(),
            );
            outcome.cursor = Some(state.cursor);
            outcome.bytes_changed = Some(result.changed);
            Ok(step(outcome, result.ops))
        }
        ExecCommand::Replace {
            scope,
            needle,
            replacement,
            allow_resize,
            force,
        } => {
            let Some((start, end)) = resolve_scope(document, state, *scope)? else {
                return Ok(step(
                    ExecOutcome::new("replace: no matches", document.is_dirty()),
                    Vec::new(),
                ));
            };
            let result = replace_range(
                document,
                start,
                end,
                needle,
                replacement,
                *allow_resize,
                *force,
            )?;
            let outcome = match result {
                ReplaceResult::Applied(outcome) => outcome,
                ReplaceResult::NoMatches => {
                    return Ok(step(
                        ExecOutcome::new("replace: no matches", document.is_dirty()),
                        Vec::new(),
                    ));
                }
                ReplaceResult::TooManyMatches { limit } => {
                    return Err(HxError::CommandError(format!(
                        "replace found more than {limit} matches; set force = true to apply"
                    )));
                }
            };
            state.cursor = clamp_cursor(document, outcome.first_match);
            if *allow_resize {
                state.selection = None;
            }
            let mut exec_outcome = ExecOutcome::new(
                format!(
                    "replaced {} matches; total {} bytes",
                    outcome.stats.match_count, outcome.stats.after_bytes
                ),
                document.is_dirty(),
            );
            exec_outcome.cursor = Some(state.cursor);
            exec_outcome.bytes_changed = Some(outcome.stats.changed_bytes as u64);
            Ok(step(exec_outcome, outcome.ops))
        }
        ExecCommand::ExportBinary { scope, path } => {
            let Some((start, end)) = resolve_scope(document, state, *scope)? else {
                return Ok(step(
                    ExecOutcome::new("exported 0 bytes", document.is_dirty()),
                    Vec::new(),
                ));
            };
            let export = export_binary_range(document, start, end, path)?;
            let mut outcome = ExecOutcome::new(
                format!(
                    "exported {} logical bytes to {}",
                    export.bytes_written,
                    path.display()
                ),
                document.is_dirty(),
            );
            outcome.bytes_read = Some(export.bytes_written);
            outcome.artifacts.push(ExecArtifact::File(export.path));
            Ok(step(outcome, Vec::new()))
        }
        ExecCommand::Save { path } => {
            let (saved, profile) = document.save(path.clone())?;
            state.cursor = clamp_cursor(document, state.cursor);
            validate_or_clear_selection(document, state)?;
            let mut outcome =
                ExecOutcome::new(format!("wrote {} [{}]", saved.display(), profile), false);
            outcome.artifacts.push(ExecArtifact::File(saved));
            Ok(StepResult {
                outcome,
                ops: Vec::new(),
                saved: true,
            })
        }
    }
}

fn step(outcome: ExecOutcome, ops: Vec<EditOp>) -> StepResult {
    StepResult {
        outcome,
        ops,
        saved: false,
    }
}

fn resolve_offset(document: &Document, state: &ExecState, offset: ExecOffset) -> HxResult<u64> {
    match offset {
        ExecOffset::Absolute(offset) => Ok(offset),
        ExecOffset::Cursor(delta) => {
            let cursor = i128::from(state.cursor);
            let target = cursor + i128::from(delta);
            u64::try_from(target).map_err(|_| HxError::OffsetOutOfRange)
        }
        ExecOffset::End => {
            if document.is_empty() {
                Ok(0)
            } else {
                Ok(document.len() - 1)
            }
        }
    }
}

fn resolve_scope(
    document: &Document,
    state: &ExecState,
    scope: ExecScope,
) -> HxResult<Option<(u64, u64)>> {
    match scope {
        ExecScope::Selection => state
            .selection
            .ok_or(HxError::MissingSelection)?
            .range
            .display_bounds(document),
        ExecScope::Range(range) => range.display_bounds(document),
        ExecScope::All => {
            if document.is_empty() {
                Ok(None)
            } else {
                Ok(Some((0, document.len() - 1)))
            }
        }
    }
}

fn display_end_for_match(document: &Document, start: u64, logical_len: u64) -> HxResult<u64> {
    if logical_len == 0 {
        return Ok(start);
    }
    let logical_start = document
        .logical_offset_for_display_offset(start)
        .ok_or(HxError::OffsetOutOfRange)?;
    document
        .display_offset_for_logical_offset(logical_start + logical_len - 1)
        .ok_or(HxError::OffsetOutOfRange)
}

fn clear_logical_selection_after_tombstone(state: &mut ExecState) {
    if state
        .selection
        .is_some_and(|selection| selection.range.space == RangeSpace::Logical)
    {
        state.selection = None;
    }
}

fn validate_or_clear_selection(document: &Document, state: &mut ExecState) -> HxResult<()> {
    if let Some(selection) = state.selection {
        if selection.range.display_bounds(document).is_err() {
            state.selection = None;
        }
    }
    Ok(())
}

fn clamp_cursor(document: &Document, offset: u64) -> u64 {
    if document.is_empty() {
        0
    } else {
        offset.min(document.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::config::Config;

    use super::*;

    fn document_with_bytes(bytes: &[u8]) -> Document {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        Document::open(file.path(), &Config::default()).unwrap()
    }

    #[test]
    fn tombstone_delete_keeps_display_selection_but_clears_logical_selection() {
        let mut display_doc = document_with_bytes(b"abcd");
        let mut display_state = ExecState::new(
            0,
            Some(ExecSelection {
                range: ExecRange::display(1, 2),
            }),
        );
        let report = execute_batch(
            &mut display_doc,
            &mut display_state,
            &[ExecCommand::Delete {
                scope: ExecScope::Selection,
                kind: DeleteKind::Tombstone,
            }],
            ExecBatchOptions::default(),
        )
        .unwrap();
        assert_eq!(
            display_state.selection.map(|selection| selection.range),
            Some(ExecRange::display(1, 2))
        );
        assert_eq!(report.undo_steps.len(), 1);

        let mut logical_doc = document_with_bytes(b"abcd");
        let mut logical_state = ExecState::new(
            0,
            Some(ExecSelection {
                range: ExecRange::logical(1, 2),
            }),
        );
        execute_batch(
            &mut logical_doc,
            &mut logical_state,
            &[ExecCommand::Delete {
                scope: ExecScope::Selection,
                kind: DeleteKind::Tombstone,
            }],
            ExecBatchOptions::default(),
        )
        .unwrap();
        assert_eq!(logical_state.selection, None);
    }

    #[test]
    fn real_insert_clears_selection() {
        let mut document = document_with_bytes(b"abcd");
        let mut state = ExecState::new(
            0,
            Some(ExecSelection {
                range: ExecRange::display(1, 2),
            }),
        );

        execute_batch(
            &mut document,
            &mut state,
            &[ExecCommand::Insert {
                offset: ExecOffset::Absolute(2),
                bytes: vec![0xaa],
            }],
            ExecBatchOptions::default(),
        )
        .unwrap();

        assert_eq!(state.selection, None);
        assert_eq!(
            document.logical_bytes(0, 4).unwrap(),
            vec![b'a', b'b', 0xaa, b'c', b'd']
        );
    }
}
