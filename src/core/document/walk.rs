use crate::core::document::Document;
use crate::core::piece_table::{CellId, PieceSource};
use crate::error::HxResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkControl {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WalkStats {
    pub pieces: usize,
    pub chunks: usize,
    pub fast_chunks: usize,
    pub slow_chunks: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LogicalChunk<'a> {
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VisibleCell {
    pub id: CellId,
    pub display_offset: u64,
    pub byte: u8,
    pub deleted: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VisibleByteSegment<'a> {
    pub display_start: u64,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VisibleCellChunk<'a> {
    pub display_start: u64,
    pub raw_bytes: &'a [u8],
    pub cells: &'a [VisibleCell],
    pub fast_path: bool,
}

impl Document {
    /// Walk display cells in piece order and emit logical byte chunks.
    ///
    /// Tombstones are skipped, replacements are applied, and inserted Add
    /// bytes are included. This is the shared primitive for streaming logical
    /// readers such as copy/export/hash and save.
    pub(crate) fn walk_logical_chunks(
        &mut self,
        start: u64,
        end_inclusive: u64,
        chunk_limit: usize,
        mut visit: impl FnMut(LogicalChunk<'_>) -> HxResult<WalkControl>,
    ) -> HxResult<WalkStats> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(WalkStats::default());
        }

        let end = end_inclusive.min(len - 1) + 1;
        let chunk_limit = self.walk_chunk_limit(chunk_limit);
        let pieces = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let mut stats = WalkStats::default();
        let mut display_cursor = 0_u64;
        let mut scratch = Vec::with_capacity(chunk_limit.min(64 * 1024));

        for piece in &pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                display_cursor = piece_end;
                continue;
            }

            stats.pieces += 1;
            let mut remaining = overlap_end - overlap_start;
            let mut source_offset = piece.start + (overlap_start - display_cursor);

            while remaining > 0 {
                let batch = (remaining as usize).min(chunk_limit);
                let raw = self.read_chunk(piece.source, source_offset, batch)?;
                if raw.is_empty() {
                    break;
                }
                let read_len = raw.len() as u64;
                let (need_tombstone_scan, need_replacement_scan) = self.overlay_flags(
                    piece.source,
                    source_offset,
                    read_len,
                    has_tombstones,
                    has_replacements,
                );
                let fast_path = !need_tombstone_scan && !need_replacement_scan;

                stats.chunks += 1;
                if fast_path {
                    stats.fast_chunks += 1;
                    let chunk = LogicalChunk { bytes: &raw };
                    if matches!(visit(chunk)?, WalkControl::Stop) {
                        return Ok(stats);
                    }
                } else {
                    stats.slow_chunks += 1;
                    let has_range_replacement = need_replacement_scan
                        && self.has_replacement_range_in_source_range(
                            piece.source,
                            source_offset,
                            read_len,
                        );

                    if need_replacement_scan && !need_tombstone_scan && !has_range_replacement {
                        scratch.clear();
                        let replacements = self.sparse_replacements_in_source_range(
                            piece.source,
                            source_offset,
                            read_len,
                        );
                        let mut cursor = 0usize;
                        for (replacement_offset, value) in replacements {
                            let idx = replacement_offset.saturating_sub(source_offset) as usize;
                            if idx > raw.len() {
                                break;
                            }
                            if cursor < idx {
                                scratch.extend_from_slice(&raw[cursor..idx]);
                            }
                            scratch.push(value);
                            cursor = idx.saturating_add(1);
                        }
                        if cursor < raw.len() {
                            scratch.extend_from_slice(&raw[cursor..]);
                        }
                    } else {
                        scratch.clear();
                        for (idx, &base) in raw.iter().enumerate() {
                            let id = CellId::from_source(piece.source, source_offset + idx as u64);
                            if need_tombstone_scan && self.is_tombstone(id) {
                                continue;
                            }
                            scratch.push(if need_replacement_scan {
                                self.replacement_for(id, base).unwrap_or(base)
                            } else {
                                base
                            });
                        }
                    }
                    let chunk = LogicalChunk { bytes: &scratch };
                    if matches!(visit(chunk)?, WalkControl::Stop) {
                        return Ok(stats);
                    }
                }

                source_offset += read_len;
                remaining -= read_len;
            }

            display_cursor = piece_end;
        }

        Ok(stats)
    }

    /// Walk visible byte segments in display order.
    ///
    /// Unlike `walk_logical_chunks`, tombstones break segments instead of
    /// being compacted away, so pattern matching never crosses a deleted
    /// display slot. Unlike `walk_visible_cells`, clean chunks are forwarded as
    /// raw bytes without building one `VisibleCell` per byte.
    pub(crate) fn walk_visible_byte_segments(
        &mut self,
        start: u64,
        end_inclusive: u64,
        chunk_limit: usize,
        mut visit: impl FnMut(VisibleByteSegment<'_>) -> HxResult<WalkControl>,
    ) -> HxResult<WalkStats> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(WalkStats::default());
        }

        let end = end_inclusive.min(len - 1) + 1;
        let chunk_limit = self.walk_chunk_limit(chunk_limit);
        let pieces = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let mut stats = WalkStats::default();
        let mut display_cursor = 0_u64;
        let mut scratch = Vec::with_capacity(chunk_limit.min(64 * 1024));

        for piece in &pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                display_cursor = piece_end;
                continue;
            }

            stats.pieces += 1;
            let mut remaining = overlap_end - overlap_start;
            let mut source_offset = piece.start + (overlap_start - display_cursor);
            let mut chunk_display_start = overlap_start;

            while remaining > 0 {
                let batch = (remaining as usize).min(chunk_limit);
                let raw = self.read_chunk(piece.source, source_offset, batch)?;
                if raw.is_empty() {
                    break;
                }
                let read_len = raw.len() as u64;
                let (need_tombstone_scan, need_replacement_scan) = self.overlay_flags(
                    piece.source,
                    source_offset,
                    read_len,
                    has_tombstones,
                    has_replacements,
                );
                let fast_path = !need_tombstone_scan && !need_replacement_scan;

                stats.chunks += 1;
                if fast_path {
                    stats.fast_chunks += 1;
                    let segment = VisibleByteSegment {
                        display_start: chunk_display_start,
                        bytes: &raw,
                    };
                    if matches!(visit(segment)?, WalkControl::Stop) {
                        return Ok(stats);
                    }
                } else {
                    stats.slow_chunks += 1;
                    let has_range_replacement = need_replacement_scan
                        && self.has_replacement_range_in_source_range(
                            piece.source,
                            source_offset,
                            read_len,
                        );

                    if has_range_replacement {
                        scratch.clear();
                        let mut segment_start = chunk_display_start;
                        for (idx, &base) in raw.iter().enumerate() {
                            let source_offset = source_offset + idx as u64;
                            let id = CellId::from_source(piece.source, source_offset);
                            if need_tombstone_scan && self.is_tombstone(id) {
                                if !scratch.is_empty() {
                                    let segment = VisibleByteSegment {
                                        display_start: segment_start,
                                        bytes: &scratch,
                                    };
                                    if matches!(visit(segment)?, WalkControl::Stop) {
                                        return Ok(stats);
                                    }
                                    scratch.clear();
                                }
                                continue;
                            }

                            if scratch.is_empty() {
                                segment_start = chunk_display_start + idx as u64;
                            }
                            scratch.push(if need_replacement_scan {
                                self.replacement_for(id, base).unwrap_or(base)
                            } else {
                                base
                            });
                        }

                        if !scratch.is_empty() {
                            let segment = VisibleByteSegment {
                                display_start: segment_start,
                                bytes: &scratch,
                            };
                            if matches!(visit(segment)?, WalkControl::Stop) {
                                return Ok(stats);
                            }
                        }
                    } else {
                        let tombstones = if need_tombstone_scan {
                            self.tombstones_in_source_range(piece.source, source_offset, read_len)
                        } else {
                            Vec::new()
                        };
                        let replacements = if need_replacement_scan {
                            self.sparse_replacements_in_source_range(
                                piece.source,
                                source_offset,
                                read_len,
                            )
                        } else {
                            Vec::new()
                        };

                        let mut cursor = 0usize;
                        let mut tombstone_idx = 0usize;
                        let mut replacement_idx = 0usize;
                        while tombstone_idx < tombstones.len()
                            || replacement_idx < replacements.len()
                        {
                            let next_tombstone = tombstones
                                .get(tombstone_idx)
                                .map(|offset| offset.saturating_sub(source_offset) as usize);
                            let next_replacement = replacements
                                .get(replacement_idx)
                                .map(|(offset, _)| offset.saturating_sub(source_offset) as usize);
                            let Some(next_dirty) =
                                Self::min_option(next_tombstone, next_replacement)
                            else {
                                break;
                            };

                            if next_dirty > cursor {
                                let segment = VisibleByteSegment {
                                    display_start: chunk_display_start + cursor as u64,
                                    bytes: &raw[cursor..next_dirty],
                                };
                                if matches!(visit(segment)?, WalkControl::Stop) {
                                    return Ok(stats);
                                }
                            }

                            let mut is_tombstone = false;
                            while tombstones.get(tombstone_idx).is_some_and(|offset| {
                                offset.saturating_sub(source_offset) as usize == next_dirty
                            }) {
                                is_tombstone = true;
                                tombstone_idx += 1;
                            }

                            let mut replacement = None;
                            while replacements
                                .get(replacement_idx)
                                .is_some_and(|(offset, _)| {
                                    offset.saturating_sub(source_offset) as usize == next_dirty
                                })
                            {
                                replacement =
                                    replacements.get(replacement_idx).map(|(_, value)| *value);
                                replacement_idx += 1;
                            }

                            if !is_tombstone {
                                if let Some(value) = replacement {
                                    scratch.clear();
                                    scratch.push(value);
                                    let segment = VisibleByteSegment {
                                        display_start: chunk_display_start + next_dirty as u64,
                                        bytes: &scratch,
                                    };
                                    if matches!(visit(segment)?, WalkControl::Stop) {
                                        return Ok(stats);
                                    }
                                }
                            }
                            cursor = next_dirty.saturating_add(1);
                        }

                        if cursor < raw.len() {
                            let segment = VisibleByteSegment {
                                display_start: chunk_display_start + cursor as u64,
                                bytes: &raw[cursor..],
                            };
                            if matches!(visit(segment)?, WalkControl::Stop) {
                                return Ok(stats);
                            }
                        }
                    }
                }

                source_offset += read_len;
                chunk_display_start += read_len;
                remaining -= read_len;
            }

            display_cursor = piece_end;
        }

        Ok(stats)
    }

    fn min_option(left: Option<usize>, right: Option<usize>) -> Option<usize> {
        match (left, right) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        }
    }

    /// Walk display cells in piece order and emit per-cell overlay state.
    ///
    /// Unlike `walk_logical_chunks`, tombstones are retained as `deleted`
    /// cells. Use this for render-ish reads, in-place transforms, and dirty
    /// search where tombstone gaps must break matches instead of being skipped.
    pub(crate) fn walk_visible_cells(
        &mut self,
        start: u64,
        end_inclusive: u64,
        chunk_limit: usize,
        mut visit: impl FnMut(&mut Document, VisibleCellChunk<'_>) -> HxResult<WalkControl>,
    ) -> HxResult<WalkStats> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(WalkStats::default());
        }

        let end = end_inclusive.min(len - 1) + 1;
        let chunk_limit = self.walk_chunk_limit(chunk_limit);
        let pieces = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let mut stats = WalkStats::default();
        let mut display_cursor = 0_u64;
        let mut cells = Vec::with_capacity(chunk_limit.min(64 * 1024));

        for piece in &pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                display_cursor = piece_end;
                continue;
            }

            stats.pieces += 1;
            let mut remaining = overlap_end - overlap_start;
            let mut source_offset = piece.start + (overlap_start - display_cursor);
            let mut chunk_display_start = overlap_start;

            while remaining > 0 {
                let batch = (remaining as usize).min(chunk_limit);
                let raw = self.read_chunk(piece.source, source_offset, batch)?;
                if raw.is_empty() {
                    break;
                }
                let read_len = raw.len() as u64;
                let (need_tombstone_scan, need_replacement_scan) = self.overlay_flags(
                    piece.source,
                    source_offset,
                    read_len,
                    has_tombstones,
                    has_replacements,
                );
                let fast_path = !need_tombstone_scan && !need_replacement_scan;

                stats.chunks += 1;
                if fast_path {
                    stats.fast_chunks += 1;
                } else {
                    stats.slow_chunks += 1;
                }

                cells.clear();
                for (idx, &base) in raw.iter().enumerate() {
                    let source_offset = source_offset + idx as u64;
                    let id = CellId::from_source(piece.source, source_offset);
                    let deleted = need_tombstone_scan && self.is_tombstone(id);
                    let byte = if !deleted && need_replacement_scan {
                        self.replacement_for(id, base).unwrap_or(base)
                    } else {
                        base
                    };
                    cells.push(VisibleCell {
                        id,
                        display_offset: chunk_display_start + idx as u64,
                        byte,
                        deleted,
                    });
                }

                let chunk = VisibleCellChunk {
                    display_start: chunk_display_start,
                    raw_bytes: &raw,
                    cells: &cells,
                    fast_path,
                };
                if matches!(visit(self, chunk)?, WalkControl::Stop) {
                    return Ok(stats);
                }

                source_offset += read_len;
                chunk_display_start += read_len;
                remaining -= read_len;
            }

            display_cursor = piece_end;
        }

        Ok(stats)
    }

    /// Reverse-order variant of `walk_visible_cells`.
    ///
    /// Chunks are visited from higher display offsets to lower offsets, while
    /// cells inside each chunk remain in forward display order. Backward search
    /// consumes `chunk.cells.iter().rev()` and can still use `raw_bytes` for
    /// memmem `rfind` on clean chunks.
    pub(crate) fn walk_visible_cells_reverse(
        &mut self,
        start: u64,
        end_inclusive: u64,
        chunk_limit: usize,
        mut visit: impl FnMut(&mut Document, VisibleCellChunk<'_>) -> HxResult<WalkControl>,
    ) -> HxResult<WalkStats> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(WalkStats::default());
        }

        let end = end_inclusive.min(len - 1) + 1;
        let chunk_limit = self.walk_chunk_limit(chunk_limit);
        let pieces = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let mut indexed_pieces = Vec::with_capacity(pieces.len());
        let mut display_cursor = 0_u64;
        for piece in pieces {
            indexed_pieces.push((piece, display_cursor));
            display_cursor += piece.len;
        }

        let mut stats = WalkStats::default();
        let mut cells = Vec::with_capacity(chunk_limit.min(64 * 1024));

        for (piece, piece_display_start) in indexed_pieces.into_iter().rev() {
            let piece_display_end = piece_display_start + piece.len;
            if piece_display_start >= end || piece_display_end <= start {
                continue;
            }

            let overlap_start = start.max(piece_display_start);
            let overlap_end = end.min(piece_display_end);
            if overlap_start >= overlap_end {
                continue;
            }

            stats.pieces += 1;
            let mut chunk_end = overlap_end;
            while chunk_end > overlap_start {
                let batch = ((chunk_end - overlap_start) as usize).min(chunk_limit);
                let chunk_display_start = chunk_end - batch as u64;
                let source_offset = piece.start + (chunk_display_start - piece_display_start);
                let raw = self.read_chunk(piece.source, source_offset, batch)?;
                if raw.is_empty() {
                    break;
                }
                let read_len = raw.len() as u64;
                let (need_tombstone_scan, need_replacement_scan) = self.overlay_flags(
                    piece.source,
                    source_offset,
                    read_len,
                    has_tombstones,
                    has_replacements,
                );
                let fast_path = !need_tombstone_scan && !need_replacement_scan;

                stats.chunks += 1;
                if fast_path {
                    stats.fast_chunks += 1;
                } else {
                    stats.slow_chunks += 1;
                }

                cells.clear();
                for (idx, &base) in raw.iter().enumerate() {
                    let source_offset = source_offset + idx as u64;
                    let id = CellId::from_source(piece.source, source_offset);
                    let deleted = need_tombstone_scan && self.is_tombstone(id);
                    let byte = if !deleted && need_replacement_scan {
                        self.replacement_for(id, base).unwrap_or(base)
                    } else {
                        base
                    };
                    cells.push(VisibleCell {
                        id,
                        display_offset: chunk_display_start + idx as u64,
                        byte,
                        deleted,
                    });
                }

                let chunk = VisibleCellChunk {
                    display_start: chunk_display_start,
                    raw_bytes: &raw,
                    cells: &cells,
                    fast_path,
                };
                if matches!(visit(self, chunk)?, WalkControl::Stop) {
                    return Ok(stats);
                }

                chunk_end = chunk_display_start;
            }
        }

        Ok(stats)
    }

    fn walk_chunk_limit(&self, requested: usize) -> usize {
        requested.max(1).min(self.max_contiguous_read_len().max(1))
    }

    fn overlay_flags(
        &self,
        source: PieceSource,
        source_offset: u64,
        len: u64,
        has_tombstones: bool,
        has_replacements: bool,
    ) -> (bool, bool) {
        if len == 0 {
            return (false, false);
        }

        let lo = CellId::from_source(source, source_offset);
        let hi = CellId::from_source(source, source_offset + len - 1);

        (
            has_tombstones && self.has_tombstone_in_range(lo, hi),
            has_replacements && self.has_replacement_in_range(lo, hi),
        )
    }
}
