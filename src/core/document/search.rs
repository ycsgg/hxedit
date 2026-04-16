use crate::core::document::{Document, SEARCH_CHUNK};
use crate::core::piece_table::{CellId, Piece};
use crate::error::{HxError, HxResult};

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

        let pieces = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let mut matcher = KmpMatcher::new(pattern);
        let mut piece_display_start = 0_u64;

        for piece in pieces {
            let piece_display_end = piece_display_start + piece.len;
            if piece_display_end <= start {
                piece_display_start = piece_display_end;
                continue;
            }

            let local_start = start.saturating_sub(piece_display_start);
            if let Some(found) = self.search_piece_forward(
                piece,
                piece_display_start,
                local_start,
                &mut matcher,
                has_tombstones,
                has_replacements,
            )? {
                return Ok(Some(found));
            }

            piece_display_start = piece_display_end;
        }

        Ok(None)
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

        let pieces = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let reversed_pattern: Vec<u8> = pattern.iter().rev().copied().collect();
        let mut matcher = KmpMatcher::new(&reversed_pattern);
        let mut indexed_pieces = Vec::with_capacity(pieces.len());
        let mut piece_display_start = 0_u64;
        for piece in pieces {
            indexed_pieces.push((piece, piece_display_start));
            piece_display_start += piece.len;
        }

        for (piece, piece_display_start) in indexed_pieces.into_iter().rev() {
            if piece_display_start >= end {
                continue;
            }

            let piece_display_end = piece_display_start + piece.len;
            let local_end = if end < piece_display_end {
                end - piece_display_start
            } else {
                piece.len
            };

            if let Some(found) = self.search_piece_backward(
                piece,
                piece_display_start,
                local_end,
                &mut matcher,
                has_tombstones,
                has_replacements,
            )? {
                return Ok(Some(found));
            }
        }

        Ok(None)
    }

    fn search_piece_forward(
        &mut self,
        piece: Piece,
        piece_display_start: u64,
        local_start: u64,
        matcher: &mut KmpMatcher<'_>,
        has_tombstones: bool,
        has_replacements: bool,
    ) -> HxResult<Option<u64>> {
        let mut remaining = piece.len.saturating_sub(local_start);
        let mut source_offset = piece.start + local_start;
        let mut display_offset = piece_display_start + local_start;

        while remaining > 0 {
            let batch = remaining.min(SEARCH_CHUNK as u64) as usize;
            let raw = self.read_chunk(piece.source, source_offset, batch)?;
            if raw.is_empty() {
                break;
            }
            let chunk_len = raw.len() as u64;
            let (need_tombstone_scan, need_replacement_scan) = self.search_overlay_flags(
                piece.source,
                source_offset,
                chunk_len,
                has_tombstones,
                has_replacements,
            );

            if !need_tombstone_scan && !need_replacement_scan {
                if let Some(found) = scan_bytes_forward(&raw, display_offset, matcher) {
                    return Ok(Some(found));
                }
            } else {
                for (idx, &base) in raw.iter().enumerate() {
                    let id = CellId::from_source(piece.source, source_offset + idx as u64);
                    if need_tombstone_scan && self.tombstones.contains(&id) {
                        matcher.reset();
                        continue;
                    }
                    let byte = if need_replacement_scan {
                        self.replacements.get(&id).copied().unwrap_or(base)
                    } else {
                        base
                    };
                    if matcher.feed(byte) {
                        return Ok(Some(
                            display_offset + idx as u64 + 1 - matcher.pattern_len(),
                        ));
                    }
                }
            }

            source_offset += chunk_len;
            display_offset += chunk_len;
            remaining -= chunk_len;
        }

        Ok(None)
    }

    fn search_piece_backward(
        &mut self,
        piece: Piece,
        piece_display_start: u64,
        local_end: u64,
        matcher: &mut KmpMatcher<'_>,
        has_tombstones: bool,
        has_replacements: bool,
    ) -> HxResult<Option<u64>> {
        let mut remaining = local_end;

        while remaining > 0 {
            let batch = remaining.min(SEARCH_CHUNK as u64) as usize;
            let chunk_start = remaining - batch as u64;
            let source_offset = piece.start + chunk_start;
            let display_offset = piece_display_start + chunk_start;

            let raw = self.read_chunk(piece.source, source_offset, batch)?;
            if raw.is_empty() {
                break;
            }
            let chunk_len = raw.len() as u64;
            let (need_tombstone_scan, need_replacement_scan) = self.search_overlay_flags(
                piece.source,
                source_offset,
                chunk_len,
                has_tombstones,
                has_replacements,
            );

            if !need_tombstone_scan && !need_replacement_scan {
                if let Some(found) = scan_bytes_backward(&raw, display_offset, matcher) {
                    return Ok(Some(found));
                }
            } else {
                for (idx, &base) in raw.iter().enumerate().rev() {
                    let id = CellId::from_source(piece.source, source_offset + idx as u64);
                    if need_tombstone_scan && self.tombstones.contains(&id) {
                        matcher.reset();
                        continue;
                    }
                    let byte = if need_replacement_scan {
                        self.replacements.get(&id).copied().unwrap_or(base)
                    } else {
                        base
                    };
                    if matcher.feed(byte) {
                        return Ok(Some(display_offset + idx as u64));
                    }
                }
            }

            remaining = chunk_start;
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
}

fn scan_bytes_forward(
    bytes: &[u8],
    display_offset: u64,
    matcher: &mut KmpMatcher<'_>,
) -> Option<u64> {
    for (idx, &byte) in bytes.iter().enumerate() {
        if matcher.feed(byte) {
            return Some(display_offset + idx as u64 + 1 - matcher.pattern_len());
        }
    }
    None
}

fn scan_bytes_backward(
    bytes: &[u8],
    display_offset: u64,
    matcher: &mut KmpMatcher<'_>,
) -> Option<u64> {
    for (idx, &byte) in bytes.iter().enumerate().rev() {
        if matcher.feed(byte) {
            return Some(display_offset + idx as u64);
        }
    }
    None
}
