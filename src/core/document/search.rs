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

        // Segment walking forwards clean chunks as raw bytes and only builds a
        // scratch segment for chunks with tombstones or replacements. Matches
        // across contiguous segment boundaries are preserved by KMP state;
        // tombstone gaps reset that state so matches never cross deleted slots.
        let overlap_keep = pattern.len() - 1;
        let mut overlap: Vec<u8> = Vec::new();
        let mut searchable: Vec<u8> = Vec::new();
        let mut found = None;
        let mut next_display_start = start;
        self.walk_visible_byte_segments(
            start,
            self.len().saturating_sub(1),
            SEARCH_CHUNK,
            |segment| {
                if segment.display_start != next_display_start {
                    overlap.clear();
                }
                next_display_start = segment.display_start + segment.bytes.len() as u64;

                if overlap.is_empty() {
                    if let Some(pos) = memchr::memmem::find(segment.bytes, pattern) {
                        found = Some(segment.display_start + pos as u64);
                        return Ok(WalkControl::Stop);
                    }

                    let keep = overlap_keep.min(segment.bytes.len());
                    overlap.extend_from_slice(&segment.bytes[segment.bytes.len() - keep..]);
                } else {
                    let base = segment.display_start - overlap.len() as u64;
                    searchable.clear();
                    searchable.extend_from_slice(&overlap);
                    searchable.extend_from_slice(segment.bytes);

                    if let Some(pos) = memchr::memmem::find(&searchable, pattern) {
                        found = Some(base + pos as u64);
                        return Ok(WalkControl::Stop);
                    }

                    let keep = overlap_keep.min(searchable.len());
                    overlap.clear();
                    overlap.extend_from_slice(&searchable[searchable.len() - keep..]);
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

    /// Backward SIMD scan over a clean document using `memmem::rfind`.
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

    fn pattern(&self) -> &'a [u8] {
        self.pattern
    }
}

/// Backward clean-chunk scanner. `matcher` is fed the reversed pattern
/// (matching the existing backward convention), so a hit reports the match's
/// start display offset directly. memmem `rfind` uses the forward pattern on
/// the forward chunk and also yields the start offset.
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
