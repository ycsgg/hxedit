use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::diff::source::{DiffByte, DiffSource};
use crate::error::HxResult;

const DEFAULT_MAX_SHIFT: usize = 256;
const DEFAULT_ANCHOR_LEN: usize = 16;
const DEFAULT_VERIFY_LEN: usize = 16;
const DEFAULT_HUNK_CAP: usize = 4096;
const DEFAULT_PREVIEW_LEN: usize = 32;
const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
const MAX_SHIFT_HARD_CAP: usize = 1024 * 1024;
const CANDIDATES_PER_HASH_CAP: usize = 32;
const TIE_FOLLOWING_SAMPLE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOptions {
    pub max_shift: usize,
    pub anchor_len: usize,
    pub verify_len: usize,
    pub hunk_cap: usize,
    pub preview_len: usize,
    pub chunk_size: usize,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            max_shift: DEFAULT_MAX_SHIFT,
            anchor_len: DEFAULT_ANCHOR_LEN,
            verify_len: DEFAULT_VERIFY_LEN,
            hunk_cap: DEFAULT_HUNK_CAP,
            preview_len: DEFAULT_PREVIEW_LEN,
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }
}

impl DiffOptions {
    pub fn normalized(mut self) -> Self {
        self.max_shift = self.max_shift.min(MAX_SHIFT_HARD_CAP);
        self.anchor_len = self.anchor_len.max(1);
        self.hunk_cap = self.hunk_cap.max(1);
        self.preview_len = self.preview_len.max(1);
        self.chunk_size = self.chunk_size.max(1);
        self
    }

    pub fn unresolved_block_len(self) -> usize {
        DEFAULT_CHUNK_SIZE
            .max(self.max_shift.saturating_mul(4))
            .max(1)
    }

    fn lookahead_len(self) -> usize {
        self.max_shift
            .saturating_add(self.anchor_len)
            .saturating_add(self.verify_len)
            .saturating_add(TIE_FOLLOWING_SAMPLE)
            .max(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffHunkKind {
    Replace,
    OnlyCurrent,
    OnlyOther,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentDiffRange {
    pub logical_start: u64,
    pub logical_len: u64,
    pub display_spans: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherDiffRange {
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub kind: DiffHunkKind,
    pub current: CurrentDiffRange,
    pub other: OtherDiffRange,
    pub current_preview: Vec<u8>,
    pub other_preview: Vec<u8>,
    pub anchor_before_display: Option<u64>,
    pub anchor_after_display: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTruncateReason {
    HunkCap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffStatus {
    Complete,
    Truncated { reason: DiffTruncateReason },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffProfile {
    pub current_bytes_scanned: u64,
    pub other_bytes_scanned: u64,
    pub chunks_read: u64,
    pub resync_attempts: u64,
    pub anchors_considered: u64,
    pub max_window_bytes: usize,
    pub hunks_emitted: usize,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub hunks: Vec<DiffHunk>,
    pub status: DiffStatus,
    pub profile: DiffProfile,
}

pub fn diff_sources<L, R>(left: L, right: R, options: DiffOptions) -> HxResult<DiffResult>
where
    L: DiffSource,
    R: DiffSource,
{
    Engine::new(left, right, options.normalized()).run()
}

struct Engine<L, R> {
    left: BufferedSource<L>,
    right: BufferedSource<R>,
    options: DiffOptions,
    hunks: Vec<DiffHunk>,
    status: DiffStatus,
    profile: DiffProfile,
}

impl<L, R> Engine<L, R>
where
    L: DiffSource,
    R: DiffSource,
{
    fn new(left: L, right: R, options: DiffOptions) -> Self {
        let chunk_size = options.chunk_size;
        Self {
            left: BufferedSource::new(left, chunk_size),
            right: BufferedSource::new(right, chunk_size),
            options,
            hunks: Vec::new(),
            status: DiffStatus::Complete,
            profile: DiffProfile::default(),
        }
    }

    fn run(mut self) -> HxResult<DiffResult> {
        let started = Instant::now();
        loop {
            self.consume_equal_run()?;
            if self.left.is_eof()? && self.right.is_eof()? {
                break;
            }
            if self.left.is_eof()? {
                self.emit_rest_only_other()?;
                break;
            }
            if self.right.is_eof()? {
                self.emit_rest_only_current()?;
                break;
            }
            if self.is_truncated() {
                break;
            }

            if self.options.max_shift == 0 {
                self.emit_strict_mismatch_run()?;
                continue;
            }

            self.profile.resync_attempts += 1;
            match self.find_anchor()? {
                Some(anchor) => {
                    let before = self.left.last_display_offset();
                    let current = self.left.consume(anchor.left_skip, &mut self.profile)?;
                    let other = self.right.consume(anchor.right_skip, &mut self.profile)?;
                    let after = self.left.peek(0)?.and_then(|byte| byte.display_offset);
                    let kind = match (current.is_empty(), other.is_empty()) {
                        (false, true) => DiffHunkKind::OnlyCurrent,
                        (true, false) => DiffHunkKind::OnlyOther,
                        (false, false) => DiffHunkKind::Replace,
                        (true, true) => continue,
                    };
                    self.push_hunk(kind, current, other, before, after);
                }
                None => self.emit_unresolved_block()?,
            }
        }

        self.profile.hunks_emitted = self.hunks.len();
        self.profile.elapsed = started.elapsed();
        Ok(DiffResult {
            hunks: self.hunks,
            status: self.status,
            profile: self.profile,
        })
    }

    fn is_truncated(&self) -> bool {
        matches!(self.status, DiffStatus::Truncated { .. })
    }

    fn consume_equal_run(&mut self) -> HxResult<()> {
        loop {
            let left = self.left.peek(0)?;
            let right = self.right.peek(0)?;
            let (Some(left), Some(right)) = (left, right) else {
                break;
            };
            if left.byte != right.byte {
                break;
            }
            self.left.consume(1, &mut self.profile)?;
            self.right.consume(1, &mut self.profile)?;
        }
        Ok(())
    }

    fn emit_strict_mismatch_run(&mut self) -> HxResult<()> {
        let before = self.left.last_display_offset();
        let mut current = Vec::new();
        let mut other = Vec::new();
        loop {
            let left = self.left.peek(0)?;
            let right = self.right.peek(0)?;
            match (left, right) {
                (Some(left), Some(right)) => {
                    if !current.is_empty() && left.byte == right.byte {
                        break;
                    }
                    if current.is_empty() && left.byte == right.byte {
                        break;
                    }
                    current.extend(self.left.consume(1, &mut self.profile)?);
                    other.extend(self.right.consume(1, &mut self.profile)?);
                }
                (Some(_), None) => {
                    current.extend(self.left.consume(1, &mut self.profile)?);
                    break;
                }
                (None, Some(_)) => {
                    other.extend(self.right.consume(1, &mut self.profile)?);
                    break;
                }
                (None, None) => break,
            }
        }
        let after = self.left.peek(0)?.and_then(|byte| byte.display_offset);
        let kind = match (current.is_empty(), other.is_empty()) {
            (false, true) => DiffHunkKind::OnlyCurrent,
            (true, false) => DiffHunkKind::OnlyOther,
            (false, false) => DiffHunkKind::Replace,
            (true, true) => return Ok(()),
        };
        self.push_hunk(kind, current, other, before, after);
        Ok(())
    }

    fn emit_unresolved_block(&mut self) -> HxResult<()> {
        let block = self.options.unresolved_block_len();
        let before = self.left.last_display_offset();
        let current = self.left.consume(block, &mut self.profile)?;
        let other = self.right.consume(block, &mut self.profile)?;
        let after = self.left.peek(0)?.and_then(|byte| byte.display_offset);
        self.push_hunk(DiffHunkKind::Unresolved, current, other, before, after);
        Ok(())
    }

    fn emit_rest_only_current(&mut self) -> HxResult<()> {
        let before = self.left.last_display_offset();
        let current = self
            .left
            .consume_all_summary(&mut self.profile, self.options.preview_len)?;
        let other = HunkSideSummary::empty_at(self.right.next_stream_offset());
        let after = self.left.peek(0)?.and_then(|byte| byte.display_offset);
        self.push_hunk_summary(DiffHunkKind::OnlyCurrent, current, other, before, after);
        Ok(())
    }

    fn emit_rest_only_other(&mut self) -> HxResult<()> {
        let before = self.left.last_display_offset();
        let current = HunkSideSummary::empty_at(self.left.next_stream_offset());
        let other = self
            .right
            .consume_all_summary(&mut self.profile, self.options.preview_len)?;
        let after = self.left.peek(0)?.and_then(|byte| byte.display_offset);
        self.push_hunk_summary(DiffHunkKind::OnlyOther, current, other, before, after);
        Ok(())
    }

    fn push_hunk(
        &mut self,
        kind: DiffHunkKind,
        current: Vec<DiffByte>,
        other: Vec<DiffByte>,
        anchor_before_display: Option<u64>,
        anchor_after_display: Option<u64>,
    ) {
        if current.is_empty() && other.is_empty() {
            return;
        }
        let current = if current.is_empty() {
            HunkSideSummary::empty_at(self.left.next_stream_offset())
        } else {
            HunkSideSummary::from_bytes(&current, self.options.preview_len)
        };
        let other = if other.is_empty() {
            HunkSideSummary::empty_at(self.right.next_stream_offset())
        } else {
            HunkSideSummary::from_bytes(&other, self.options.preview_len)
        };
        self.push_hunk_summary(
            kind,
            current,
            other,
            anchor_before_display,
            anchor_after_display,
        );
    }

    fn push_hunk_summary(
        &mut self,
        kind: DiffHunkKind,
        current: HunkSideSummary,
        other: HunkSideSummary,
        anchor_before_display: Option<u64>,
        anchor_after_display: Option<u64>,
    ) {
        if current.len == 0 && other.len == 0 {
            return;
        }
        if self.hunks.len() >= self.options.hunk_cap {
            self.status = DiffStatus::Truncated {
                reason: DiffTruncateReason::HunkCap,
            };
            return;
        }

        self.hunks.push(DiffHunk {
            kind,
            current: CurrentDiffRange {
                logical_start: current.start,
                logical_len: current.len,
                display_spans: current.display_spans,
            },
            other: OtherDiffRange {
                offset: other.start,
                len: other.len,
            },
            current_preview: current.preview,
            other_preview: other.preview,
            anchor_before_display,
            anchor_after_display,
        });
    }

    fn find_anchor(&mut self) -> HxResult<Option<AnchorCandidate>> {
        let lookahead = self.options.lookahead_len();
        self.left.fill_at_least(lookahead, &mut self.profile)?;
        self.right.fill_at_least(lookahead, &mut self.profile)?;
        self.profile.max_window_bytes = self
            .profile
            .max_window_bytes
            .max(self.left.buffer_len() + self.right.buffer_len());

        let left_window = self.left.window(lookahead);
        let right_window = self.right.window(lookahead);
        if left_window.len() < self.options.anchor_len
            || right_window.len() < self.options.anchor_len
        {
            return Ok(None);
        }

        let max_left_skip = self
            .options
            .max_shift
            .min(left_window.len().saturating_sub(1));
        let max_right_skip = self
            .options
            .max_shift
            .min(right_window.len().saturating_sub(1));
        let mut right_anchors: HashMap<u64, Vec<usize>> = HashMap::new();
        for pos in 0..=max_right_skip {
            if pos + self.options.anchor_len > right_window.len() {
                break;
            }
            let bytes = window_bytes(&right_window[pos..pos + self.options.anchor_len]);
            if low_information(&bytes) {
                continue;
            }
            let entry = right_anchors.entry(kgram_hash(&bytes)).or_default();
            if entry.len() < CANDIDATES_PER_HASH_CAP {
                entry.push(pos);
            }
        }

        let mut best: Option<AnchorCandidate> = None;
        for left_pos in 0..=max_left_skip {
            if left_pos + self.options.anchor_len > left_window.len() {
                break;
            }
            let left_anchor =
                window_bytes(&left_window[left_pos..left_pos + self.options.anchor_len]);
            if low_information(&left_anchor) {
                continue;
            }
            let Some(right_positions) = right_anchors.get(&kgram_hash(&left_anchor)) else {
                continue;
            };
            for &right_pos in right_positions {
                self.profile.anchors_considered += 1;
                if left_pos == 0 && right_pos == 0 {
                    continue;
                }
                if !equal_bytes(
                    &left_window[left_pos..left_pos + self.options.anchor_len],
                    &right_window[right_pos..right_pos + self.options.anchor_len],
                ) {
                    continue;
                }
                let following = following_equal_len(
                    &left_window[left_pos + self.options.anchor_len..],
                    &right_window[right_pos + self.options.anchor_len..],
                    self.options.verify_len + TIE_FOLLOWING_SAMPLE,
                );
                if following < self.options.verify_len {
                    continue;
                }
                let candidate = AnchorCandidate {
                    left_skip: left_pos,
                    right_skip: right_pos,
                    following_equal: following,
                };
                if best.as_ref().is_none_or(|old| candidate.better_than(old)) {
                    best = Some(candidate);
                }
            }
        }

        Ok(best)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AnchorCandidate {
    left_skip: usize,
    right_skip: usize,
    following_equal: usize,
}

impl AnchorCandidate {
    fn better_than(self, other: &Self) -> bool {
        let self_score = self.left_skip + self.right_skip;
        let other_score = other.left_skip + other.right_skip;
        self_score < other_score
            || (self_score == other_score
                && self.left_skip.abs_diff(self.right_skip)
                    < other.left_skip.abs_diff(other.right_skip))
            || (self_score == other_score
                && self.left_skip.abs_diff(self.right_skip)
                    == other.left_skip.abs_diff(other.right_skip)
                && self.following_equal > other.following_equal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HunkSideSummary {
    start: u64,
    len: u64,
    preview: Vec<u8>,
    display_spans: Vec<(u64, u64)>,
}

impl HunkSideSummary {
    fn empty_at(start: u64) -> Self {
        Self {
            start,
            len: 0,
            preview: Vec::new(),
            display_spans: Vec::new(),
        }
    }

    fn from_bytes(bytes: &[DiffByte], preview_len: usize) -> Self {
        let start = bytes.first().map(|byte| byte.stream_offset).unwrap_or(0);
        let mut summary = Self::empty_at(start);
        for &byte in bytes {
            summary.push(byte, preview_len);
        }
        summary
    }

    fn push(&mut self, byte: DiffByte, preview_len: usize) {
        if self.len == 0 {
            self.start = byte.stream_offset;
        }
        self.len += 1;
        if self.preview.len() < preview_len {
            self.preview.push(byte.byte);
        }
        if let Some(display) = byte.display_offset {
            if let Some((start, len)) = self.display_spans.last_mut() {
                if *start + *len == display {
                    *len += 1;
                    return;
                }
            }
            self.display_spans.push((display, 1));
        }
    }
}

struct BufferedSource<S> {
    source: S,
    buffer: VecDeque<DiffByte>,
    eof: bool,
    chunk_size: usize,
    next_offset: u64,
    last_display: Option<u64>,
}

impl<S> BufferedSource<S>
where
    S: DiffSource,
{
    fn new(source: S, chunk_size: usize) -> Self {
        Self {
            source,
            buffer: VecDeque::new(),
            eof: false,
            chunk_size,
            next_offset: 0,
            last_display: None,
        }
    }

    fn fill_at_least(&mut self, wanted: usize, profile: &mut DiffProfile) -> HxResult<()> {
        while !self.eof && self.buffer.len() < wanted {
            let need = (wanted - self.buffer.len()).max(1).min(self.chunk_size);
            let bytes = self.source.read_next(need)?;
            profile.chunks_read += 1;
            if bytes.is_empty() {
                self.eof = true;
                break;
            }
            for byte in bytes {
                self.next_offset = byte.stream_offset.saturating_add(1);
                self.buffer.push_back(byte);
            }
        }
        Ok(())
    }

    fn peek(&mut self, idx: usize) -> HxResult<Option<DiffByte>> {
        self.fill_at_least(idx + 1, &mut DiffProfile::default())?;
        Ok(self.buffer.get(idx).copied())
    }

    fn is_eof(&mut self) -> HxResult<bool> {
        self.fill_at_least(1, &mut DiffProfile::default())?;
        Ok(self.eof && self.buffer.is_empty())
    }

    fn consume(&mut self, count: usize, profile: &mut DiffProfile) -> HxResult<Vec<DiffByte>> {
        self.fill_at_least(count, profile)?;
        let actual = count.min(self.buffer.len());
        let mut out = Vec::with_capacity(actual);
        for _ in 0..actual {
            if let Some(byte) = self.buffer.pop_front() {
                if let Some(display) = byte.display_offset {
                    self.last_display = Some(display);
                    profile.current_bytes_scanned += 1;
                } else {
                    profile.other_bytes_scanned += 1;
                }
                out.push(byte);
            }
        }
        Ok(out)
    }

    fn consume_all_summary(
        &mut self,
        profile: &mut DiffProfile,
        preview_len: usize,
    ) -> HxResult<HunkSideSummary> {
        let mut out = HunkSideSummary::empty_at(self.next_stream_offset());
        loop {
            self.fill_at_least(self.chunk_size, profile)?;
            if self.buffer.is_empty() {
                break;
            }
            let count = self.buffer.len();
            for byte in self.consume(count, profile)? {
                out.push(byte, preview_len);
            }
        }
        Ok(out)
    }

    fn window(&self, count: usize) -> Vec<DiffByte> {
        self.buffer.iter().take(count).copied().collect()
    }

    fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    fn next_stream_offset(&self) -> u64 {
        self.buffer
            .front()
            .map(|byte| byte.stream_offset)
            .unwrap_or(self.next_offset)
    }

    fn last_display_offset(&self) -> Option<u64> {
        self.last_display
    }
}

fn window_bytes(window: &[DiffByte]) -> Vec<u8> {
    window.iter().map(|byte| byte.byte).collect()
}

fn equal_bytes(left: &[DiffByte], right: &[DiffByte]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| left.byte == right.byte)
}

fn following_equal_len(left: &[DiffByte], right: &[DiffByte], limit: usize) -> usize {
    left.iter()
        .zip(right.iter())
        .take(limit)
        .take_while(|(left, right)| left.byte == right.byte)
        .count()
}

fn low_information(bytes: &[u8]) -> bool {
    bytes
        .first()
        .is_some_and(|first| bytes.iter().all(|byte| byte == first))
}

fn kgram_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::source::{DiffByte, DiffSource, VecDiffSource};
    use crate::error::HxResult;

    fn small_options(max_shift: usize) -> DiffOptions {
        DiffOptions {
            max_shift,
            anchor_len: 2,
            verify_len: 0,
            hunk_cap: 4096,
            preview_len: 32,
            chunk_size: 8,
        }
    }

    #[test]
    fn diff_equal_files_has_no_hunks() {
        let result = diff_sources(
            VecDiffSource::current(b"abc".to_vec()),
            VecDiffSource::other(b"abc".to_vec()),
            small_options(2),
        )
        .unwrap();
        assert!(result.hunks.is_empty());
        assert_eq!(result.status, DiffStatus::Complete);
    }

    #[test]
    fn diff_strict_n_zero_reports_shift_as_mismatch() {
        let result = diff_sources(
            VecDiffSource::current(vec![0xaa, 0xbb, 0x11, 0x22, 0xcc, 0xdd]),
            VecDiffSource::other(vec![0xaa, 0xbb, 0xcc, 0xdd]),
            small_options(0),
        )
        .unwrap();
        assert_eq!(result.hunks.len(), 2);
        assert_eq!(result.hunks[0].kind, DiffHunkKind::Replace);
        assert_eq!(result.hunks[0].current.logical_len, 3);
        assert_eq!(result.hunks[0].other.len, 2);
        assert_eq!(result.hunks[1].kind, DiffHunkKind::OnlyCurrent);
        assert_eq!(result.hunks[1].current.logical_len, 1);
    }

    #[test]
    fn diff_resyncs_after_current_only_bytes_within_n() {
        let result = diff_sources(
            VecDiffSource::current(vec![0xaa, 0xbb, 0x11, 0x22, 0xcc, 0xdd]),
            VecDiffSource::other(vec![0xaa, 0xbb, 0xcc, 0xdd]),
            small_options(2),
        )
        .unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.hunks[0].kind, DiffHunkKind::OnlyCurrent);
        assert_eq!(result.hunks[0].current.logical_start, 2);
        assert_eq!(result.hunks[0].current.logical_len, 2);
        assert_eq!(result.hunks[0].other.len, 0);
    }

    #[test]
    fn diff_resyncs_after_other_only_bytes_within_n() {
        let result = diff_sources(
            VecDiffSource::current(vec![0xaa, 0xbb, 0xcc, 0xdd]),
            VecDiffSource::other(vec![0xaa, 0xbb, 0x11, 0x22, 0xcc, 0xdd]),
            small_options(2),
        )
        .unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.hunks[0].kind, DiffHunkKind::OnlyOther);
        assert_eq!(result.hunks[0].current.logical_len, 0);
        assert_eq!(result.hunks[0].other.offset, 2);
        assert_eq!(result.hunks[0].other.len, 2);
        assert_eq!(result.hunks[0].anchor_after_display, Some(2));
    }

    #[test]
    fn diff_resyncs_after_replace_then_equal_anchor() {
        let result = diff_sources(
            VecDiffSource::current(vec![1, 2, 9, 9, 3, 4, 5]),
            VecDiffSource::other(vec![1, 2, 8, 8, 3, 4, 5]),
            small_options(2),
        )
        .unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.hunks[0].kind, DiffHunkKind::Replace);
        assert_eq!(result.hunks[0].current.logical_len, 2);
        assert_eq!(result.hunks[0].other.len, 2);
    }

    #[test]
    fn diff_marks_shift_larger_than_n_unresolved() {
        let result = diff_sources(
            VecDiffSource::current(vec![1, 2, 9, 9, 9, 3, 4]),
            VecDiffSource::other(vec![1, 2, 3, 4]),
            small_options(2),
        )
        .unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.hunks[0].kind, DiffHunkKind::Unresolved);
    }

    #[test]
    fn diff_does_not_false_resync_on_short_repeated_zero_anchor() {
        let mut options = small_options(4);
        options.anchor_len = 4;
        let result = diff_sources(
            VecDiffSource::current(vec![1, 0, 0, 0, 0, 2, 3]),
            VecDiffSource::other(vec![1, 9, 9, 0, 0, 0, 0, 3]),
            options,
        )
        .unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.hunks[0].kind, DiffHunkKind::Unresolved);
    }

    #[test]
    fn diff_tracks_current_display_spans_across_tombstone_gaps() {
        struct GapSource {
            bytes: Vec<DiffByte>,
            index: usize,
        }
        impl DiffSource for GapSource {
            fn read_next(&mut self, max_bytes: usize) -> HxResult<Vec<DiffByte>> {
                let end = (self.index + max_bytes).min(self.bytes.len());
                let out = self.bytes[self.index..end].to_vec();
                self.index = end;
                Ok(out)
            }
        }

        let current = GapSource {
            bytes: vec![
                DiffByte {
                    stream_offset: 0,
                    display_offset: Some(0),
                    byte: 0xaa,
                },
                DiffByte {
                    stream_offset: 1,
                    display_offset: Some(2),
                    byte: 0xbb,
                },
                DiffByte {
                    stream_offset: 2,
                    display_offset: Some(3),
                    byte: 0xcc,
                },
            ],
            index: 0,
        };
        let result = diff_sources(
            current,
            VecDiffSource::other(vec![0x11, 0x22, 0xcc]),
            small_options(0),
        )
        .unwrap();
        assert_eq!(result.hunks[0].current.display_spans, vec![(0, 1), (2, 1)]);
    }

    #[test]
    fn diff_large_virtual_file_resyncs_without_materializing_inputs() {
        #[derive(Clone)]
        struct PatternSource {
            len: usize,
            insert_at: Option<usize>,
            insert: Vec<u8>,
            index: usize,
            current: bool,
        }
        impl PatternSource {
            fn byte_at(&self, idx: usize) -> u8 {
                if let Some(insert_at) = self.insert_at {
                    if idx >= insert_at && idx < insert_at + self.insert.len() {
                        return self.insert[idx - insert_at];
                    }
                    let base_idx = if idx >= insert_at + self.insert.len() {
                        idx - self.insert.len()
                    } else {
                        idx
                    };
                    return pattern_byte(base_idx);
                }
                pattern_byte(idx)
            }
        }
        impl DiffSource for PatternSource {
            fn read_next(&mut self, max_bytes: usize) -> HxResult<Vec<DiffByte>> {
                if self.index >= self.len {
                    return Ok(Vec::new());
                }
                let end = (self.index + max_bytes).min(self.len);
                let start = self.index;
                self.index = end;
                Ok((start..end)
                    .map(|idx| DiffByte {
                        stream_offset: idx as u64,
                        display_offset: self.current.then_some(idx as u64),
                        byte: self.byte_at(idx),
                    })
                    .collect())
            }
        }
        fn pattern_byte(idx: usize) -> u8 {
            (idx.wrapping_mul(31).wrapping_add(idx >> 8) & 0xff) as u8
        }

        let base_len = 16 * 1024 * 1024;
        let insert = vec![0xde, 0xad, 0xbe, 0xef, 0xaa, 0x55, 0x42, 0x24];
        let current = PatternSource {
            len: base_len + insert.len(),
            insert_at: Some(8 * 1024 * 1024),
            insert: insert.clone(),
            index: 0,
            current: true,
        };
        let other = PatternSource {
            len: base_len,
            insert_at: None,
            insert: Vec::new(),
            index: 0,
            current: false,
        };
        let options = DiffOptions {
            max_shift: insert.len(),
            chunk_size: 64 * 1024,
            ..DiffOptions::default()
        };
        let result = diff_sources(current, other, options).unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.hunks[0].kind, DiffHunkKind::OnlyCurrent);
        assert_eq!(result.hunks[0].current.logical_len, insert.len() as u64);
        assert!(result.profile.max_window_bytes < 200_000);
        assert!(result.profile.current_bytes_scanned >= base_len as u64);
    }

    #[test]
    fn diff_large_files_all_different_emits_bounded_unresolved() {
        let len = 2 * 1024 * 1024;
        let current = VecDiffSource::current(vec![0xaa; len]);
        let other = VecDiffSource::other(vec![0x55; len]);
        let options = DiffOptions {
            max_shift: 64,
            hunk_cap: 128,
            ..DiffOptions::default()
        };
        let result = diff_sources(current, other, options).unwrap();
        assert!(result.hunks.len() <= 32);
        assert!(result
            .hunks
            .iter()
            .all(|hunk| hunk.kind == DiffHunkKind::Unresolved));
    }
}
