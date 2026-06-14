use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::Arc;

use crate::core::piece_table::{CellId, PieceSource};

#[derive(Debug, Clone, PartialEq, Eq)]
enum SparseReplacement {
    Value(u8),
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RangeReplacement {
    Pattern { pattern: Arc<[u8]>, phase: u64 },
    Xor { key: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplacementRange {
    start: CellId,
    len: u64,
    value: RangeReplacement,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReplacementStore {
    sparse: BTreeMap<CellId, SparseReplacement>,
    ranges: BTreeMap<CellId, ReplacementRange>,
}

impl ReplacementStore {
    pub(crate) fn is_empty(&self) -> bool {
        self.sparse.is_empty() && self.ranges.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.sparse.clear();
        self.ranges.clear();
    }

    pub(crate) fn get(&self, id: CellId, base: u8) -> Option<u8> {
        match self.sparse.get(&id) {
            Some(SparseReplacement::Value(value)) => return Some(*value),
            Some(SparseReplacement::Clear) => return None,
            None => {}
        }

        self.range_containing(id)
            .map(|range| range.value_at(id, base))
    }

    pub(crate) fn set_cell(&mut self, id: CellId, value: u8) {
        self.sparse.insert(id, SparseReplacement::Value(value));
    }

    pub(crate) fn clear_cell(&mut self, id: CellId) {
        if self.range_containing(id).is_some() {
            self.sparse.insert(id, SparseReplacement::Clear);
        } else {
            self.sparse.remove(&id);
        }
    }

    pub(crate) fn set_pattern_range(
        &mut self,
        source: PieceSource,
        start: u64,
        len: u64,
        pattern: Arc<[u8]>,
        phase: u64,
    ) {
        if len == 0 || pattern.is_empty() {
            return;
        }
        self.clear_source_range(source, start, len);
        let id = CellId::from_source(source, start);
        self.ranges.insert(
            id,
            ReplacementRange {
                start: id,
                len,
                value: RangeReplacement::Pattern { pattern, phase },
            },
        );
    }

    pub(crate) fn set_xor_range(&mut self, source: PieceSource, start: u64, len: u64, key: u8) {
        if len == 0 || key == 0 {
            return;
        }
        self.clear_source_range(source, start, len);
        let id = CellId::from_source(source, start);
        self.ranges.insert(
            id,
            ReplacementRange {
                start: id,
                len,
                value: RangeReplacement::Xor { key },
            },
        );
    }

    pub(crate) fn clear_source_range(&mut self, source: PieceSource, start: u64, len: u64) {
        if len == 0 {
            return;
        }
        let end = start.saturating_add(len);
        self.sparse
            .retain(|id, _| !cell_in_source_range(*id, source, start, end));

        let keys = self.overlapping_range_keys(source, start, end);
        for key in keys {
            let Some(range) = self.ranges.remove(&key) else {
                continue;
            };
            let range_start = range.source_offset();
            let range_end = range.end();

            if range_start < start {
                let left_len = start - range_start;
                let mut left = range.clone();
                left.len = left_len;
                self.ranges.insert(left.start, left);
            }

            if end < range_end {
                let right_start = end;
                let right_len = range_end - end;
                let mut right = range.shifted(right_start, right_len);
                right.start = CellId::from_source(source, right_start);
                self.ranges.insert(right.start, right);
            }
        }
    }

    pub(crate) fn has_in_range(&self, lo: CellId, hi: CellId) -> bool {
        if lo > hi {
            return false;
        }
        if self
            .sparse
            .range((Bound::Included(lo), Bound::Included(hi)))
            .next()
            .is_some()
        {
            return true;
        }

        let Some((source, start, end)) = normalized_cell_range(lo, hi) else {
            return self.has_in_range_same_source(source_of(lo), offset_of(lo), u64::MAX)
                || self.has_in_range_same_source(
                    source_of(hi),
                    0,
                    offset_of(hi).saturating_add(1),
                );
        };
        self.has_in_range_same_source(source, start, end)
    }

    pub(crate) fn dirty_bytes(&self) -> usize {
        let mut total = self
            .ranges
            .values()
            .map(|range| range.len as usize)
            .sum::<usize>();

        for (id, value) in &self.sparse {
            if self.range_containing(*id).is_some() {
                if matches!(value, SparseReplacement::Clear) {
                    total = total.saturating_sub(1);
                }
            } else if matches!(value, SparseReplacement::Value(_)) {
                total = total.saturating_add(1);
            }
        }

        total
    }

    pub(crate) fn sparse_values(&self) -> impl Iterator<Item = (CellId, u8)> + '_ {
        self.sparse.iter().filter_map(|(id, value)| match value {
            SparseReplacement::Value(value) => Some((*id, *value)),
            SparseReplacement::Clear => None,
        })
    }

    pub(crate) fn has_range_at(&self, id: CellId) -> bool {
        self.range_containing(id).is_some()
    }

    pub(crate) fn range_snapshots(&self) -> Vec<ReplacementRangeSnapshot> {
        self.ranges
            .values()
            .map(|range| ReplacementRangeSnapshot {
                source: range.source(),
                source_offset: range.source_offset(),
                len: range.len,
            })
            .collect()
    }

    fn range_containing(&self, id: CellId) -> Option<&ReplacementRange> {
        let (_, range) = self.ranges.range(..=id).next_back()?;
        range.contains(id).then_some(range)
    }

    fn has_in_range_same_source(&self, source: PieceSource, start: u64, end: u64) -> bool {
        if start >= end {
            return false;
        }
        !self.overlapping_range_keys(source, start, end).is_empty()
    }

    fn overlapping_range_keys(&self, source: PieceSource, start: u64, end: u64) -> Vec<CellId> {
        if start >= end {
            return Vec::new();
        }

        let mut keys = Vec::new();
        let start_id = CellId::from_source(source, start);
        if let Some((key, range)) = self.ranges.range(..=start_id).next_back() {
            if range.overlaps(source, start, end) {
                keys.push(*key);
            }
        }

        let source_end = CellId::from_source(source, u64::MAX);
        for (key, range) in self
            .ranges
            .range((Bound::Included(start_id), Bound::Included(source_end)))
        {
            if range.source() != source {
                continue;
            }
            if range.source_offset() >= end {
                break;
            }
            if !keys.contains(key) && range.overlaps(source, start, end) {
                keys.push(*key);
            }
        }

        keys
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ReplacementRangeSnapshot {
    pub(crate) source: PieceSource,
    pub(crate) source_offset: u64,
    pub(crate) len: u64,
}

impl ReplacementRange {
    fn source(&self) -> PieceSource {
        source_of(self.start)
    }

    fn source_offset(&self) -> u64 {
        offset_of(self.start)
    }

    fn end(&self) -> u64 {
        self.source_offset().saturating_add(self.len)
    }

    fn contains(&self, id: CellId) -> bool {
        source_of(id) == self.source() && {
            let offset = offset_of(id);
            offset >= self.source_offset() && offset < self.end()
        }
    }

    fn overlaps(&self, source: PieceSource, start: u64, end: u64) -> bool {
        self.source() == source && self.source_offset() < end && start < self.end()
    }

    fn value_at(&self, id: CellId, base: u8) -> u8 {
        match &self.value {
            RangeReplacement::Pattern { pattern, phase } => {
                let local = offset_of(id) - self.source_offset();
                let pattern_len = pattern.len() as u64;
                let index = ((*phase % pattern_len) + (local % pattern_len)) % pattern_len;
                pattern[index as usize]
            }
            RangeReplacement::Xor { key } => base ^ key,
        }
    }

    fn shifted(&self, start: u64, len: u64) -> Self {
        let delta = start - self.source_offset();
        let value = match &self.value {
            RangeReplacement::Pattern { pattern, phase } => {
                let pattern_len = pattern.len() as u64;
                RangeReplacement::Pattern {
                    pattern: Arc::clone(pattern),
                    phase: ((*phase % pattern_len) + (delta % pattern_len)) % pattern_len,
                }
            }
            RangeReplacement::Xor { key } => RangeReplacement::Xor { key: *key },
        };
        Self {
            start: CellId::from_source(self.source(), start),
            len,
            value,
        }
    }
}

fn normalized_cell_range(lo: CellId, hi: CellId) -> Option<(PieceSource, u64, u64)> {
    let source = source_of(lo);
    if source != source_of(hi) {
        return None;
    }
    Some((source, offset_of(lo), offset_of(hi).saturating_add(1)))
}

fn cell_in_source_range(id: CellId, source: PieceSource, start: u64, end: u64) -> bool {
    source_of(id) == source && {
        let offset = offset_of(id);
        offset >= start && offset < end
    }
}

fn source_of(id: CellId) -> PieceSource {
    match id {
        CellId::Original(_) => PieceSource::Original,
        CellId::Add(_) => PieceSource::Add,
    }
}

fn offset_of(id: CellId) -> u64 {
    match id {
        CellId::Original(offset) | CellId::Add(offset) => offset,
    }
}
