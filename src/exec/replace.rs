use crate::core::document::walk::WalkControl;
use crate::core::document::Document;
use crate::error::{HxError, HxResult};

use super::{BulkReplacement, EditOp};

const REPLACE_CONFIRM_LIMIT: usize = 65_535;
const REPLACE_BATCH_LIMIT: usize = 65_535;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplaceStats {
    pub(crate) match_count: usize,
    pub(crate) before_bytes: usize,
    pub(crate) after_bytes: usize,
    pub(crate) changed_bytes: usize,
}

#[derive(Debug)]
pub(crate) struct ReplaceOutcome {
    pub(crate) first_match: u64,
    pub(crate) ops: Vec<EditOp>,
    pub(crate) stats: ReplaceStats,
}

#[derive(Debug)]
pub(crate) enum ReplaceResult {
    Applied(ReplaceOutcome),
    NoMatches,
    TooManyMatches { limit: usize },
}

struct ReplaceMatchCollector<'a> {
    pattern: &'a [u8],
    limit: usize,
    matches: Vec<u64>,
    tail: Vec<u8>,
    tail_start: u64,
    next_start: u64,
}

impl<'a> ReplaceMatchCollector<'a> {
    fn new(pattern: &'a [u8], limit: usize, start: u64) -> Self {
        Self {
            pattern,
            limit,
            matches: Vec::new(),
            tail: Vec::with_capacity(pattern.len().saturating_sub(1)),
            tail_start: start,
            next_start: start,
        }
    }

    fn is_full(&self) -> bool {
        self.matches.len() >= self.limit
    }

    fn finish(self) -> (Vec<u64>, u64) {
        (self.matches, self.next_start)
    }

    fn feed_segment(&mut self, display_start: u64, bytes: &[u8]) {
        if bytes.is_empty() || self.is_full() {
            return;
        }
        if !self.tail.is_empty()
            && self.tail_start.saturating_add(self.tail.len() as u64) != display_start
        {
            self.tail.clear();
            self.tail_start = display_start;
        }

        let base = if self.tail.is_empty() {
            display_start
        } else {
            self.tail_start
        };
        let mut searchable = Vec::with_capacity(self.tail.len() + bytes.len());
        searchable.extend_from_slice(&self.tail);
        searchable.extend_from_slice(bytes);

        let pattern_len = self.pattern.len();
        let pattern_len_u64 = pattern_len as u64;
        let mut scan_pos = self
            .next_start
            .saturating_sub(base)
            .min(searchable.len() as u64) as usize;

        while scan_pos + pattern_len <= searchable.len() && !self.is_full() {
            let Some(relative) = memchr::memmem::find(&searchable[scan_pos..], self.pattern) else {
                break;
            };
            let found_pos = scan_pos + relative;
            let found = base + found_pos as u64;
            self.matches.push(found);
            self.next_start = found.saturating_add(pattern_len_u64);
            scan_pos = found_pos + pattern_len;
        }

        let min_tail_index = self
            .next_start
            .saturating_sub(base)
            .min(searchable.len() as u64) as usize;
        let suffix_start = searchable
            .len()
            .saturating_sub(pattern_len.saturating_sub(1));
        let tail_start_index = suffix_start.max(min_tail_index);
        self.tail.clear();
        self.tail.extend_from_slice(&searchable[tail_start_index..]);
        self.tail_start = base + tail_start_index as u64;
    }
}

pub(crate) fn replace_range(
    document: &mut Document,
    start: u64,
    end_inclusive: u64,
    needle: &[u8],
    replacement: &[u8],
    allow_resize: bool,
    force: bool,
) -> HxResult<ReplaceResult> {
    if needle.is_empty() {
        return Err(HxError::InvalidReplace(
            "needle must not be empty".to_owned(),
        ));
    }
    if !allow_resize && needle.len() != replacement.len() {
        return Err(HxError::InvalidReplace(
            "equal-length replace requires same-size needle/replacement; use :re! to resize"
                .to_owned(),
        ));
    }
    if document.is_empty() {
        return Ok(ReplaceResult::NoMatches);
    }

    if needle.len() == replacement.len() {
        return apply_replace_same_size_streaming(
            document,
            start,
            end_inclusive,
            needle,
            replacement,
            force,
        );
    }

    if !force {
        let (matches, _) = collect_replace_match_batch(
            document,
            start,
            end_inclusive,
            needle,
            REPLACE_CONFIRM_LIMIT + 1,
        )?;
        if matches.is_empty() {
            return Ok(ReplaceResult::NoMatches);
        }
        if matches.len() > REPLACE_CONFIRM_LIMIT {
            return Ok(ReplaceResult::TooManyMatches {
                limit: REPLACE_CONFIRM_LIMIT,
            });
        }
        return apply_replace_resizing(document, &matches, needle, replacement)
            .map(ReplaceResult::Applied);
    }

    let matches = collect_replace_matches(document, start, end_inclusive, needle)?;
    if matches.is_empty() {
        return Ok(ReplaceResult::NoMatches);
    }
    apply_replace_resizing(document, &matches, needle, replacement).map(ReplaceResult::Applied)
}

fn collect_replace_matches(
    document: &mut Document,
    start: u64,
    end_inclusive: u64,
    needle: &[u8],
) -> HxResult<Vec<u64>> {
    if start > end_inclusive || needle.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    let mut search_start = start;
    let end = end_inclusive.min(document.len().saturating_sub(1));

    while search_start <= end {
        let Some(found) = document.search_forward(search_start, needle)? else {
            break;
        };
        let found_end = found + needle.len() as u64 - 1;
        if found > end || found_end > end {
            break;
        }

        matches.push(found);
        search_start = found.saturating_add(needle.len() as u64);
    }

    Ok(matches)
}

fn collect_replace_match_batch(
    document: &mut Document,
    start: u64,
    end_inclusive: u64,
    needle: &[u8],
    limit: usize,
) -> HxResult<(Vec<u64>, u64)> {
    if start > end_inclusive || needle.is_empty() || limit == 0 {
        return Ok((Vec::new(), start));
    }

    let end = end_inclusive.min(document.len().saturating_sub(1));
    if start > end || end.saturating_sub(start) + 1 < needle.len() as u64 {
        return Ok((Vec::new(), start));
    }

    let mut collector = ReplaceMatchCollector::new(needle, limit, start);
    document.walk_visible_byte_segments(start, end, 64 * 1024, |segment| {
        collector.feed_segment(segment.display_start, segment.bytes);
        Ok(if collector.is_full() {
            WalkControl::Stop
        } else {
            WalkControl::Continue
        })
    })?;

    let (matches, next_start) = collector.finish();
    let filtered = matches
        .into_iter()
        .take_while(|offset| {
            offset
                .checked_add(needle.len() as u64 - 1)
                .is_some_and(|found_end| found_end <= end)
        })
        .collect::<Vec<_>>();
    let next_start = filtered
        .last()
        .map(|offset| offset.saturating_add(needle.len() as u64))
        .unwrap_or(next_start);
    Ok((filtered, next_start))
}

fn apply_replace_same_size_streaming(
    document: &mut Document,
    start: u64,
    end_inclusive: u64,
    needle: &[u8],
    replacement: &[u8],
    force: bool,
) -> HxResult<ReplaceResult> {
    if !force {
        let (matches, _) = collect_replace_match_batch(
            document,
            start,
            end_inclusive,
            needle,
            REPLACE_CONFIRM_LIMIT + 1,
        )?;
        if matches.is_empty() {
            return Ok(ReplaceResult::NoMatches);
        }
        if matches.len() > REPLACE_CONFIRM_LIMIT {
            return Ok(ReplaceResult::TooManyMatches {
                limit: REPLACE_CONFIRM_LIMIT,
            });
        }
        let outcome = apply_replace_same_size_matches(document, &matches, needle, replacement)?;
        return Ok(ReplaceResult::Applied(outcome));
    }

    let mut all_ops = Vec::new();
    let mut stats = ReplaceStats {
        match_count: 0,
        before_bytes: 0,
        after_bytes: 0,
        changed_bytes: 0,
    };
    let mut first_match = None;
    let mut search_start = start;

    loop {
        let (matches, next_start) = collect_replace_match_batch(
            document,
            search_start,
            end_inclusive,
            needle,
            REPLACE_BATCH_LIMIT,
        )?;
        if matches.is_empty() {
            break;
        }

        first_match.get_or_insert(matches[0]);
        let outcome = apply_replace_same_size_matches(document, &matches, needle, replacement)?;
        all_ops.extend(outcome.ops);
        stats.match_count += outcome.stats.match_count;
        stats.before_bytes += outcome.stats.before_bytes;
        stats.after_bytes += outcome.stats.after_bytes;
        stats.changed_bytes += outcome.stats.changed_bytes;

        if next_start <= search_start || next_start > end_inclusive {
            break;
        }
        search_start = next_start;
    }

    let Some(first_match) = first_match else {
        return Ok(ReplaceResult::NoMatches);
    };

    Ok(ReplaceResult::Applied(ReplaceOutcome {
        first_match,
        ops: all_ops,
        stats,
    }))
}

fn apply_replace_same_size_matches(
    document: &mut Document,
    matches: &[u64],
    needle: &[u8],
    replacement: &[u8],
) -> HxResult<ReplaceOutcome> {
    let first_match = matches[0];
    if needle == replacement {
        return Ok(ReplaceOutcome {
            first_match,
            ops: Vec::new(),
            stats: ReplaceStats {
                match_count: matches.len(),
                before_bytes: matches.len() * needle.len(),
                after_bytes: matches.len() * replacement.len(),
                changed_bytes: 0,
            },
        });
    }

    let needle_len = needle.len() as u64;
    let mut ops = Vec::new();
    let mut changed_bytes = 0usize;

    let mut run_start = matches[0];
    let mut run_matches = 1usize;
    let mut previous = matches[0];
    for &offset in &matches[1..] {
        if offset == previous + needle_len {
            run_matches += 1;
        } else {
            changed_bytes += apply_replace_same_size_run(
                document,
                run_start,
                run_matches,
                needle_len,
                replacement,
                &mut ops,
            )?;
            run_start = offset;
            run_matches = 1;
        }
        previous = offset;
    }
    changed_bytes += apply_replace_same_size_run(
        document,
        run_start,
        run_matches,
        needle_len,
        replacement,
        &mut ops,
    )?;

    Ok(ReplaceOutcome {
        first_match,
        ops,
        stats: ReplaceStats {
            match_count: matches.len(),
            before_bytes: matches.len() * needle.len(),
            after_bytes: matches.len() * replacement.len(),
            changed_bytes,
        },
    })
}

fn apply_replace_same_size_run(
    document: &mut Document,
    offset: u64,
    match_count: usize,
    needle_len: u64,
    replacement: &[u8],
    ops: &mut Vec<EditOp>,
) -> HxResult<usize> {
    let run_len = needle_len
        .checked_mul(match_count as u64)
        .ok_or(HxError::OffsetOutOfRange)?;
    if document.replacement_range_is_pristine(offset, run_len) {
        document.overwrite_run_pattern_overlay(offset, run_len, replacement)?;
        ops.push(EditOp::ReplaceBulk {
            offset,
            len: run_len,
            before: BulkReplacement::Clear,
            after: BulkReplacement::Pattern(replacement.to_vec()),
        });
        return Ok(run_len as usize);
    }

    if document.display_range_has_tombstone(offset, run_len) {
        return Err(HxError::OffsetOutOfRange);
    }
    let before = document.replacement_patch_for_display_range(offset, run_len)?;
    let stats = document.overwrite_run_pattern_overlay(offset, run_len, replacement)?;
    let after = document.replacement_patch_for_display_range(offset, stats.visited)?;

    let changed = if before == after {
        0
    } else {
        stats.changed as usize
    };
    if before != after {
        ops.push(EditOp::ReplacePatch {
            offset,
            len: stats.visited,
            before,
            after,
        });
    }
    Ok(changed)
}

fn apply_replace_resizing(
    document: &mut Document,
    matches: &[u64],
    needle: &[u8],
    replacement: &[u8],
) -> HxResult<ReplaceOutcome> {
    let mut ops = Vec::new();

    for &offset in matches.iter().rev() {
        let removed = document.delete_range_real(offset, needle.len() as u64)?;
        if !removed.is_empty() {
            ops.push(EditOp::RealDelete {
                offset,
                cells: removed,
            });
        }

        let inserted = document.insert_bytes(offset, replacement)?;
        if !inserted.is_empty() {
            ops.push(EditOp::Insert {
                offset,
                cells: inserted,
            });
        }
    }

    Ok(ReplaceOutcome {
        first_match: matches[0],
        ops,
        stats: ReplaceStats {
            match_count: matches.len(),
            before_bytes: matches.len() * needle.len(),
            after_bytes: matches.len() * replacement.len(),
            changed_bytes: matches.len() * needle.len().max(replacement.len()),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::config::Config;
    use crate::exec::undo_edit_op;

    use super::*;

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
    fn same_size_replace_uses_replacement_semantics_and_undoes() {
        let mut document = document_with_bytes(b"abab");

        let result = replace_range(&mut document, 0, 3, b"ab", b"cd", false, false).unwrap();
        let ReplaceResult::Applied(outcome) = result else {
            panic!("expected replace to apply");
        };

        assert_eq!(outcome.stats.match_count, 2);
        assert_eq!(logical_all(&mut document), b"cdcd");
        assert_eq!(document.len(), 4);
        assert!(matches!(
            outcome.ops.as_slice(),
            [EditOp::ReplaceBulk { .. }]
        ));
        for op in outcome.ops.iter().rev() {
            undo_edit_op(&mut document, op).unwrap();
        }
        assert_eq!(logical_all(&mut document), b"abab");
        assert_eq!(document.len(), 4);
    }

    #[test]
    fn resize_replace_uses_real_delete_insert_and_undoes() {
        let mut document = document_with_bytes(b"abcabc");

        let result = replace_range(&mut document, 0, 5, b"b", b"XYZ", true, false).unwrap();
        let ReplaceResult::Applied(outcome) = result else {
            panic!("expected replace to apply");
        };

        assert_eq!(outcome.stats.match_count, 2);
        assert_eq!(logical_all(&mut document), b"aXYZcaXYZc");
        assert_eq!(document.len(), 10);
        assert!(outcome
            .ops
            .iter()
            .any(|op| matches!(op, EditOp::RealDelete { .. })));
        assert!(outcome
            .ops
            .iter()
            .any(|op| matches!(op, EditOp::Insert { .. })));
        for op in outcome.ops.iter().rev() {
            undo_edit_op(&mut document, op).unwrap();
        }
        assert_eq!(logical_all(&mut document), b"abcabc");
        assert_eq!(document.len(), 6);
    }
}
