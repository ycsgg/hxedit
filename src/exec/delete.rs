use crate::core::document::Document;
use crate::error::HxResult;

use super::EditOp;

#[derive(Debug)]
pub(crate) struct DeleteResult {
    pub(crate) deleted: u64,
    pub(crate) ops: Vec<EditOp>,
}

/// Tombstone-delete one display cell. The display slot remains present and is
/// skipped by logical readers/save.
pub(crate) fn tombstone_delete_at(document: &mut Document, offset: u64) -> HxResult<DeleteResult> {
    let Some(id) = document.delete_byte(offset)? else {
        return Ok(DeleteResult {
            deleted: 0,
            ops: Vec::new(),
        });
    };
    Ok(DeleteResult {
        deleted: 1,
        ops: vec![EditOp::TombstoneDelete { ids: vec![id] }],
    })
}

/// Tombstone-delete a display range. Existing tombstones are left unchanged and
/// do not produce undo entries.
pub(crate) fn tombstone_delete_range(
    document: &mut Document,
    start: u64,
    end_inclusive: u64,
) -> HxResult<DeleteResult> {
    if start > end_inclusive {
        return Ok(DeleteResult {
            deleted: 0,
            ops: Vec::new(),
        });
    }

    let span = end_inclusive - start + 1;
    let candidates = document.cell_ids_range(start, span);
    let mut ids = Vec::with_capacity(candidates.len());
    for id in candidates {
        if document.is_tombstone(id) {
            continue;
        }
        document.mark_tombstones(&[id])?;
        ids.push(id);
    }

    Ok(DeleteResult {
        deleted: ids.len() as u64,
        ops: vec![EditOp::TombstoneDelete { ids }],
    })
}

/// Real-delete bytes from the piece table. Following display offsets shift
/// left, so callers must only use this for explicit layout-changing paths.
pub(crate) fn real_delete_range(
    document: &mut Document,
    offset: u64,
    len: u64,
) -> HxResult<DeleteResult> {
    let cells = document.delete_range_real(offset, len)?;
    Ok(DeleteResult {
        deleted: cells.len() as u64,
        ops: vec![EditOp::RealDelete { offset, cells }],
    })
}
