use std::sync::Arc;

use crate::core::document::{Document, ReplacementPatch};
use crate::core::piece_table::CellId;
use crate::error::{HxError, HxResult};

/// Snapshot of a single cell's replacement state before an edit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplacementChange {
    pub(crate) id: CellId,
    /// `None` means the cell had no replacement (base byte was displayed).
    pub(crate) before: Option<u8>,
    /// `None` means the cell returns to its base byte.
    pub(crate) after: Option<u8>,
}

/// Compact replacement state used by bulk undo records.
///
/// These variants intentionally describe replacement-only effects over a
/// display range. They never insert, tombstone, or real-delete bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BulkReplacement {
    /// No replacement entries are present in the range.
    Clear,
    /// Repeating overwrite pattern, starting at the first display cell.
    Pattern(Vec<u8>),
    /// Exact overwrite bytes, starting at the first display cell.
    Bytes(Arc<[u8]>),
    /// XOR every visible byte in the range with the given key.
    Xor { key: u8 },
}

/// A single reversible edit operation.
///
/// Each variant stores enough information to undo itself:
/// - `Insert` — undo by real-deleting `len` bytes at `offset`.
/// - `RealDelete` — undo by re-inserting the saved `cells` at `offset`.
/// - `TombstoneDelete` — undo by clearing the tombstones for `ids`.
/// - `ReplaceBytes` — undo by restoring each cell's previous replacement.
/// - `ReplaceBulk` — undo/redo by applying compact replacement recipes to a
///   display range whose pre-edit replacement state was proven simple.
/// - `ReplacePatch` — undo/redo by restoring a run-based replacement snapshot
///   for dirty or mixed ranges without expanding one entry per byte.
#[derive(Debug, Clone)]
pub(crate) enum EditOp {
    Insert {
        offset: u64,
        cells: Vec<CellId>,
    },
    RealDelete {
        offset: u64,
        cells: Vec<CellId>,
    },
    TombstoneDelete {
        ids: Vec<CellId>,
    },
    ReplaceBytes {
        changes: Vec<ReplacementChange>,
    },
    ReplaceBulk {
        offset: u64,
        len: u64,
        before: BulkReplacement,
        after: BulkReplacement,
    },
    ReplacePatch {
        offset: u64,
        len: u64,
        before: ReplacementPatch,
        after: ReplacementPatch,
    },
}

pub(crate) fn apply_edit_op(document: &mut Document, op: &EditOp) -> HxResult<()> {
    match op {
        EditOp::Insert { offset, cells } => document.restore_real_delete(*offset, cells)?,
        EditOp::RealDelete { offset, cells } => {
            let removed = document.delete_range_real(*offset, cells.len() as u64)?;
            debug_assert_eq!(removed, *cells);
        }
        EditOp::TombstoneDelete { ids } => document.mark_tombstones(ids)?,
        EditOp::ReplaceBytes { changes } => {
            for change in changes {
                document.restore_replacement(change.id, change.after)?;
            }
        }
        EditOp::ReplaceBulk {
            offset, len, after, ..
        } => apply_bulk_replacement(document, *offset, *len, after)?,
        EditOp::ReplacePatch {
            offset, len, after, ..
        } => document.restore_replacement_patch_in_display_range(*offset, *len, after)?,
    }
    Ok(())
}

pub(crate) fn undo_edit_op(document: &mut Document, op: &EditOp) -> HxResult<()> {
    match op {
        EditOp::Insert { offset, cells } => {
            let removed = document.delete_range_real(*offset, cells.len() as u64)?;
            debug_assert_eq!(removed, *cells);
        }
        EditOp::RealDelete { offset, cells } => document.restore_real_delete(*offset, cells)?,
        EditOp::TombstoneDelete { ids } => document.clear_tombstones(ids),
        EditOp::ReplaceBytes { changes } => {
            for change in changes {
                document.restore_replacement(change.id, change.before)?;
            }
        }
        EditOp::ReplaceBulk {
            offset,
            len,
            before,
            ..
        } => apply_bulk_replacement(document, *offset, *len, before)?,
        EditOp::ReplacePatch {
            offset,
            len,
            before,
            ..
        } => document.restore_replacement_patch_in_display_range(*offset, *len, before)?,
    }
    Ok(())
}

pub(crate) fn edit_op_has_effect(op: &EditOp) -> bool {
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

fn apply_bulk_replacement(
    document: &mut Document,
    offset: u64,
    len: u64,
    replacement: &BulkReplacement,
) -> HxResult<()> {
    let end = offset.checked_add(len).ok_or(HxError::OffsetOutOfRange)?;
    if end > document.len() {
        return Err(HxError::OffsetOutOfRange);
    }

    match replacement {
        BulkReplacement::Clear => document.clear_replacements_in_display_range(offset, len),
        BulkReplacement::Pattern(pattern) => {
            if len == 0 {
                return Ok(());
            }
            if pattern.is_empty() {
                return Err(HxError::CommandError(
                    "bulk replacement pattern must not be empty".to_owned(),
                ));
            }
            document.overwrite_run_pattern_overlay(offset, len, pattern)?;
            Ok(())
        }
        BulkReplacement::Bytes(bytes) => {
            if len == 0 {
                return Ok(());
            }
            if bytes.len() as u64 != len {
                return Err(HxError::CommandError(
                    "bulk replacement byte run length mismatch".to_owned(),
                ));
            }
            document.overwrite_run_bytes_overlay(offset, Arc::clone(bytes))?;
            Ok(())
        }
        BulkReplacement::Xor { key } => {
            if len == 0 {
                return Ok(());
            }
            document.xor_visible_range_overlay(offset, offset + len - 1, *key)?;
            Ok(())
        }
    }
}
