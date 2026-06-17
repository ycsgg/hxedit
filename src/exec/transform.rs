use crate::core::document::Document;
use crate::error::{HxError, HxResult};

use super::{BulkReplacement, EditOp, ReplacementChange};

#[derive(Debug)]
pub(crate) struct OverwriteResult {
    pub(crate) written: u64,
    pub(crate) ops: Vec<EditOp>,
}

#[derive(Debug)]
pub(crate) struct FillResult {
    pub(crate) written: u64,
    pub(crate) changed: u64,
    pub(crate) ops: Vec<EditOp>,
}

#[derive(Debug)]
pub(crate) struct XorResult {
    pub(crate) visited: u64,
    pub(crate) changed: u64,
    pub(crate) ops: Vec<EditOp>,
}

#[derive(Debug)]
pub(crate) struct InsertResult {
    pub(crate) inserted: usize,
    pub(crate) ops: Vec<EditOp>,
}

/// Overwrite bytes at a display offset. Pure replacement semantics: this never
/// inserts, tombstones, or real-deletes. Bytes beyond EOF are truncated.
pub(crate) fn overwrite_bytes(
    document: &mut Document,
    offset: u64,
    bytes: &[u8],
) -> HxResult<OverwriteResult> {
    if document.is_readonly() {
        return Err(HxError::ReadOnly);
    }
    if bytes.is_empty() {
        return Ok(OverwriteResult {
            written: 0,
            ops: Vec::new(),
        });
    }

    let doc_len = document.len();
    let applied = bytes.len().min(doc_len.saturating_sub(offset) as usize);
    let use_bulk_undo = document.replacement_range_is_pristine(offset, applied as u64);

    let ops = if use_bulk_undo {
        let (_written, runs) =
            document.overwrite_run_bytes_overlay_changed(offset, &bytes[..applied])?;
        runs.into_iter()
            .map(|(offset, bytes)| EditOp::ReplaceBulk {
                offset,
                len: bytes.len() as u64,
                before: BulkReplacement::Clear,
                after: BulkReplacement::Bytes(bytes),
            })
            .collect()
    } else {
        let len = applied as u64;
        if document.display_range_has_tombstone(offset, len) {
            return Err(HxError::OffsetOutOfRange);
        }
        let before = document.replacement_patch_for_display_range(offset, len)?;
        document.overwrite_run_bytes_overlay_changed(offset, &bytes[..applied])?;
        let after = document.replacement_patch_for_display_range(offset, len)?;
        if before == after {
            Vec::new()
        } else {
            vec![EditOp::ReplacePatch {
                offset,
                len,
                before,
                after,
            }]
        }
    };

    Ok(OverwriteResult {
        written: applied as u64,
        ops,
    })
}

/// Insert bytes at a display offset. Real insert semantics: following display
/// offsets shift right.
pub(crate) fn insert_bytes(
    document: &mut Document,
    offset: u64,
    bytes: &[u8],
) -> HxResult<InsertResult> {
    if document.is_readonly() {
        return Err(HxError::ReadOnly);
    }
    if bytes.is_empty() {
        return Ok(InsertResult {
            inserted: 0,
            ops: Vec::new(),
        });
    }

    let inserted = document.insert_bytes(offset, bytes)?;
    Ok(InsertResult {
        inserted: bytes.len(),
        ops: vec![EditOp::Insert {
            offset,
            cells: inserted,
        }],
    })
}

/// Replace exact bytes at a display offset, returning a per-cell replacement
/// undo record. Pure replacement semantics; bytes outside the current display
/// length are ignored.
pub(crate) fn replace_bytes_at(
    document: &mut Document,
    offset: u64,
    bytes: &[u8],
) -> HxResult<Vec<EditOp>> {
    let mut changes = Vec::new();
    for (index, &byte) in bytes.iter().enumerate() {
        let target = offset + index as u64;
        if target >= document.len() {
            continue;
        }
        let Some(id) = document.cell_id_at(target) else {
            continue;
        };
        let before = document.replacement_state(id)?;
        document.replace_display_byte_by_id(id, byte)?;
        let after = document.replacement_state(id)?;
        if after != before {
            changes.push(ReplacementChange { id, before, after });
        }
    }

    if changes.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![EditOp::ReplaceBytes { changes }])
    }
}

/// Overwrite-fill a display run with a repeating pattern. Pure replacement
/// semantics; the write is truncated at EOF.
pub(crate) fn fill_overwrite(
    document: &mut Document,
    offset: u64,
    pattern: &[u8],
    run_len: u64,
) -> HxResult<FillResult> {
    if document.is_readonly() {
        return Err(HxError::ReadOnly);
    }
    if pattern.is_empty() || run_len == 0 {
        return Ok(FillResult {
            written: 0,
            changed: 0,
            ops: Vec::new(),
        });
    }

    let doc_len = document.len();
    let applied = if offset >= doc_len {
        0
    } else {
        run_len.min(doc_len - offset)
    };
    let use_bulk_undo = document.replacement_range_is_pristine(offset, applied);

    let (written, changed, ops) = if use_bulk_undo {
        let stats = document.overwrite_run_pattern_overlay(offset, run_len, pattern)?;
        let ops = if stats.changed == 0 {
            Vec::new()
        } else {
            vec![EditOp::ReplaceBulk {
                offset,
                len: stats.visited,
                before: BulkReplacement::Clear,
                after: BulkReplacement::Pattern(pattern.to_vec()),
            }]
        };
        (stats.visited, stats.changed, ops)
    } else {
        if document.display_range_has_tombstone(offset, applied) {
            return Err(HxError::OffsetOutOfRange);
        }
        let before = document.replacement_patch_for_display_range(offset, applied)?;
        let stats = document.overwrite_run_pattern_overlay(offset, run_len, pattern)?;
        let after = document.replacement_patch_for_display_range(offset, stats.visited)?;
        let changed = if before == after { 0 } else { stats.changed };
        let ops = if before == after {
            Vec::new()
        } else {
            vec![EditOp::ReplacePatch {
                offset,
                len: stats.visited,
                before,
                after,
            }]
        };
        (stats.visited, changed, ops)
    };

    Ok(FillResult {
        written,
        changed,
        ops,
    })
}

/// XOR visible logical bytes in a display range in place. Pure replacement
/// semantics; tombstones are skipped.
pub(crate) fn xor_in_place(
    document: &mut Document,
    start: u64,
    end_inclusive: u64,
    key: u8,
) -> HxResult<XorResult> {
    if document.is_readonly() {
        return Err(HxError::ReadOnly);
    }

    let range_len = end_inclusive.saturating_sub(start).saturating_add(1);
    let use_bulk_undo = key != 0 && document.replacement_range_is_pristine(start, range_len);
    let (visited, changed, ops) = if use_bulk_undo {
        let stats = document.xor_visible_range_overlay(start, end_inclusive, key)?;
        let ops = if stats.changed == 0 {
            Vec::new()
        } else {
            vec![EditOp::ReplaceBulk {
                offset: start,
                len: stats.visited,
                before: BulkReplacement::Clear,
                after: BulkReplacement::Xor { key },
            }]
        };
        (stats.visited, stats.changed, ops)
    } else {
        let before = document.replacement_patch_for_display_range(start, range_len)?;
        let stats = document.xor_visible_range_mixed_overlay(start, end_inclusive, key)?;
        let after = document.replacement_patch_for_display_range(start, range_len)?;
        let changed = if before == after { 0 } else { stats.changed };
        let ops = if before == after {
            Vec::new()
        } else {
            vec![EditOp::ReplacePatch {
                offset: start,
                len: range_len,
                before,
                after,
            }]
        };
        (stats.visited, changed, ops)
    };

    Ok(XorResult {
        visited,
        changed,
        ops,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::config::Config;

    use super::*;
    use crate::exec::undo_edit_op;

    fn document_with_bytes(bytes: &[u8]) -> Document {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        Document::open(file.path(), &Config::default()).unwrap()
    }

    fn logical_all(document: &mut Document) -> Vec<u8> {
        if document.is_empty() {
            Vec::new()
        } else {
            document.logical_bytes(0, document.len() - 1).unwrap()
        }
    }

    #[test]
    fn overwrite_bytes_is_replacement_only_and_undoable() {
        let mut document = document_with_bytes(b"abcdef");

        let result = overwrite_bytes(&mut document, 2, b"XY").unwrap();

        assert_eq!(result.written, 2);
        assert_eq!(logical_all(&mut document), b"abXYef");
        assert_eq!(document.len(), 6);
        for op in result.ops.iter().rev() {
            undo_edit_op(&mut document, op).unwrap();
        }
        assert_eq!(logical_all(&mut document), b"abcdef");
        assert_eq!(document.len(), 6);
    }

    #[test]
    fn replace_bytes_at_is_replacement_only_and_undoable() {
        let mut document = document_with_bytes(b"abcdef");

        let ops = replace_bytes_at(&mut document, 1, b"XY").unwrap();

        assert_eq!(logical_all(&mut document), b"aXYdef");
        assert_eq!(document.len(), 6);
        assert!(matches!(ops.as_slice(), [EditOp::ReplaceBytes { .. }]));
        for op in ops.iter().rev() {
            undo_edit_op(&mut document, op).unwrap();
        }
        assert_eq!(logical_all(&mut document), b"abcdef");
        assert_eq!(document.len(), 6);
    }

    #[test]
    fn insert_bytes_shifts_display_offsets_and_is_undoable() {
        let mut document = document_with_bytes(b"abef");

        let result = insert_bytes(&mut document, 2, b"cd").unwrap();

        assert_eq!(result.inserted, 2);
        assert_eq!(logical_all(&mut document), b"abcdef");
        assert_eq!(document.len(), 6);
        for op in result.ops.iter().rev() {
            undo_edit_op(&mut document, op).unwrap();
        }
        assert_eq!(logical_all(&mut document), b"abef");
        assert_eq!(document.len(), 4);
    }

    #[test]
    fn xor_in_place_skips_tombstones() {
        let mut document = document_with_bytes(&[0x10, 0x20, 0x30]);
        document.mark_tombstone(1).unwrap();

        let result = xor_in_place(&mut document, 0, 2, 0xff).unwrap();

        assert_eq!(result.visited, 2);
        assert_eq!(logical_all(&mut document), vec![0xef, 0xcf]);
        for op in result.ops.iter().rev() {
            undo_edit_op(&mut document, op).unwrap();
        }
        assert_eq!(logical_all(&mut document), vec![0x10, 0x30]);
    }
}
