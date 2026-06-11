use crate::core::document::{Document, SEARCH_CHUNK};
use crate::error::{HxError, HxResult};

use super::walk::WalkControl;

impl Document {
    /// Search forward through the display stream. Tombstoned bytes break
    /// matches (they are treated as gaps). Inserted bytes participate normally.
    pub fn search_forward(&mut self, start: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        if pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        if start >= self.len() {
            return Ok(None);
        }

        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();

        // Clean documents (no tombstones / replacements) have a contiguous
        // display stream, so we can scan it in 64 KB chunks with SIMD memmem
        // instead of the byte-at-a-time KMP loop. Cross-chunk matches are
        // handled by keeping a `pattern.len() - 1` overlap. Dirty documents
        // fall back to the KMP path below, which preserves tombstone gaps and
        // replacement overlays.
        if !has_tombstones && !has_replacements {
            return self.search_clean_forward(start, pattern);
        }

        let mut matcher = KmpMatcher::new(pattern);
        let mut found = None;
        self.walk_visible_cells(
            start,
            self.len().saturating_sub(1),
            SEARCH_CHUNK,
            |_, chunk| {
                if chunk.fast_path {
                    if let Some(hit) =
                        scan_clean_chunk_forward(chunk.raw_bytes, chunk.display_start, &mut matcher)
                    {
                        found = Some(hit);
                        return Ok(WalkControl::Stop);
                    }
                    return Ok(WalkControl::Continue);
                }

                for cell in chunk.cells {
                    if cell.deleted {
                        matcher.reset();
                        continue;
                    }
                    if matcher.feed(cell.byte) {
                        found = Some(cell.display_offset + 1 - matcher.pattern_len());
                        return Ok(WalkControl::Stop);
                    }
                }

                Ok(WalkControl::Continue)
            },
        )?;

        Ok(found)
    }

    /// Search backward through the display stream.
    pub fn search_backward(&mut self, end_exclusive: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        if pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        let end = end_exclusive.min(self.len());
        if end == 0 {
            return Ok(None);
        }

        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();

        // Clean documents scan backward with SIMD memmem rfind in 64 KB
        // chunks; dirty documents keep the KMP fallback (see `search_forward`).
        if !has_tombstones && !has_replacements {
            return self.search_clean_backward(end, pattern);
        }

        let reversed_pattern: Vec<u8> = pattern.iter().rev().copied().collect();
        let mut matcher = KmpMatcher::new(&reversed_pattern);
        let mut found = None;
        self.walk_visible_cells_reverse(0, end - 1, SEARCH_CHUNK, |_, chunk| {
            if chunk.fast_path {
                if let Some(hit) =
                    scan_clean_chunk_backward(chunk.raw_bytes, chunk.display_start, &mut matcher)
                {
                    found = Some(hit);
                    return Ok(WalkControl::Stop);
                }
                return Ok(WalkControl::Continue);
            }

            for cell in chunk.cells.iter().rev() {
                if cell.deleted {
                    matcher.reset();
                    continue;
                }
                if matcher.feed(cell.byte) {
                    found = Some(cell.display_offset);
                    return Ok(WalkControl::Stop);
                }
            }

            Ok(WalkControl::Continue)
        })?;

        Ok(found)
    }

    /// Forward SIMD scan over a clean (no tombstone / replacement) document.
    ///
    /// The display stream is contiguous, so `read_logical_range` returns the
    /// exact display bytes at a display offset. We scan in chunks bounded by
    /// the page-cache capacity and keep a `pattern.len() - 1` byte overlap so
    /// matches straddling a chunk (or piece) boundary are still found.
    fn search_clean_forward(&mut self, start: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        let end = self.len();
        if end.saturating_sub(start) < pattern.len() as u64 {
            return Ok(None);
        }

        let chunk_len = self
            .max_contiguous_read_len()
            .min(SEARCH_CHUNK)
            .max(pattern.len());
        let overlap_keep = pattern.len() - 1;
        let mut cursor = start;
        let mut overlap: Vec<u8> = Vec::new();

        while cursor < end {
            let want = ((end - cursor) as usize).min(chunk_len);
            let chunk = self.read_logical_range(cursor, want)?;
            if chunk.is_empty() {
                break;
            }
            let read_len = chunk.len() as u64;
            // `base` is the display offset of the first byte in `searchable`.
            let base = cursor - overlap.len() as u64;
            let mut searchable = overlap;
            searchable.extend_from_slice(&chunk);

            if let Some(pos) = memchr::memmem::find(&searchable, pattern) {
                let found = base + pos as u64;
                if found >= start {
                    return Ok(Some(found));
                }
            }

            let keep = overlap_keep.min(searchable.len());
            overlap = searchable[searchable.len() - keep..].to_vec();
            cursor += read_len;
        }

        Ok(None)
    }

    /// Backward SIMD scan over a clean document (mirror of
    /// [`search_clean_forward`] using `memmem::rfind`).
    fn search_clean_backward(&mut self, end: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        if end < pattern.len() as u64 {
            return Ok(None);
        }

        let chunk_len = self
            .max_contiguous_read_len()
            .min(SEARCH_CHUNK)
            .max(pattern.len());
        let overlap_keep = pattern.len() - 1;
        let mut cursor = end;
        let mut overlap: Vec<u8> = Vec::new();

        while cursor > 0 {
            let want = (cursor as usize).min(chunk_len);
            let chunk_start = cursor - want as u64;
            let chunk = self.read_logical_range(chunk_start, want)?;
            if chunk.is_empty() {
                break;
            }
            let mut searchable = chunk;
            searchable.extend_from_slice(&overlap);

            if let Some(pos) = memchr::memmem::rfind(&searchable, pattern) {
                let found = chunk_start + pos as u64;
                if found + pattern.len() as u64 <= end {
                    return Ok(Some(found));
                }
            }

            let keep = overlap_keep.min(searchable.len());
            overlap = searchable[..keep].to_vec();
            cursor = chunk_start;
        }

        Ok(None)
    }
}

#[derive(Debug)]
struct KmpMatcher<'a> {
    pattern: &'a [u8],
    prefix: Vec<usize>,
    matched: usize,
}

impl<'a> KmpMatcher<'a> {
    fn new(pattern: &'a [u8]) -> Self {
        let mut prefix = vec![0; pattern.len()];
        let mut matched = 0;
        for idx in 1..pattern.len() {
            while matched > 0 && pattern[idx] != pattern[matched] {
                matched = prefix[matched - 1];
            }
            if pattern[idx] == pattern[matched] {
                matched += 1;
                prefix[idx] = matched;
            }
        }

        Self {
            pattern,
            prefix,
            matched: 0,
        }
    }

    fn feed(&mut self, byte: u8) -> bool {
        while self.matched > 0 && byte != self.pattern[self.matched] {
            self.matched = self.prefix[self.matched - 1];
        }

        if byte == self.pattern[self.matched] {
            self.matched += 1;
            if self.matched == self.pattern.len() {
                self.matched = self.prefix[self.matched - 1];
                return true;
            }
        }

        false
    }

    fn reset(&mut self) {
        self.matched = 0;
    }

    fn pattern_len(&self) -> u64 {
        self.pattern.len() as u64
    }

    fn pattern(&self) -> &'a [u8] {
        self.pattern
    }
}

/// Scan a clean chunk forward, semantically equivalent to feeding every byte
/// of `bytes` into `matcher` (returning the first complete match's start
/// display offset), but using SIMD memmem for the bulk of the work.
///
/// Three steps keep it equivalent to the byte-at-a-time loop:
/// 1. Feed the first `P-1` bytes through `matcher` to complete any match that
///    straddles the previous chunk boundary (carried in `matcher.matched`).
/// 2. Find the earliest match fully inside this chunk with `memmem::find`.
///    A start-of-chunk (`pos == 0`) internal match is never completed in step
///    1 (it only feeds `P-1` bytes), so memmem owns all internal matches; the
///    two steps neither overlap nor miss.
/// 3. Reset `matcher` and feed the trailing `P-1` bytes so the next chunk can
///    detect a match straddling this boundary.
fn scan_clean_chunk_forward(
    bytes: &[u8],
    display_offset: u64,
    matcher: &mut KmpMatcher<'_>,
) -> Option<u64> {
    let pattern = matcher.pattern();
    let p = pattern.len();
    let n = bytes.len();

    let head = p.saturating_sub(1).min(n);
    for (i, &byte) in bytes[..head].iter().enumerate() {
        if matcher.feed(byte) {
            return Some(display_offset + i as u64 + 1 - p as u64);
        }
    }

    if n >= p {
        if let Some(pos) = memchr::memmem::find(bytes, pattern) {
            return Some(display_offset + pos as u64);
        }
    }

    matcher.reset();
    let tail = p.saturating_sub(1).min(n);
    for &byte in &bytes[n - tail..] {
        // tail < p, so feed can never complete a match here.
        matcher.feed(byte);
    }
    None
}

/// Backward mirror of [`scan_clean_chunk_forward`]. `matcher` is fed the
/// reversed pattern (matching the existing backward convention), so a hit
/// reports the match's start display offset directly. memmem `rfind` uses the
/// forward pattern on the forward chunk and also yields the start offset.
fn scan_clean_chunk_backward(
    bytes: &[u8],
    display_offset: u64,
    matcher: &mut KmpMatcher<'_>,
) -> Option<u64> {
    // matcher holds the reversed pattern; recover the forward pattern for
    // memmem by reversing it back.
    let reversed = matcher.pattern();
    let p = reversed.len();
    let n = bytes.len();
    let forward_pattern: Vec<u8> = reversed.iter().rev().copied().collect();

    // Step 1: feed the trailing P-1 bytes (from the end) to complete a match
    // straddling the following chunk boundary.
    let head = p.saturating_sub(1).min(n);
    for offset in 0..head {
        let idx = n - 1 - offset;
        if matcher.feed(bytes[idx]) {
            return Some(display_offset + idx as u64);
        }
    }

    if n >= p {
        if let Some(pos) = memchr::memmem::rfind(bytes, &forward_pattern) {
            return Some(display_offset + pos as u64);
        }
    }

    matcher.reset();
    let tail = p.saturating_sub(1).min(n);
    for &byte in bytes[..tail].iter().rev() {
        matcher.feed(byte);
    }
    None
}
