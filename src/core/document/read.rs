use crate::core::document::{ByteSlot, Document};
use crate::core::piece_table::{CellId, Piece, PieceSource};
use crate::error::HxResult;

const LOGICAL_CHUNK: usize = 64 * 1024;

impl Document {
    pub fn raw_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if offset >= self.original_len {
            return Ok(Vec::new());
        }
        let clamped = len.min((self.original_len - offset) as usize);
        self.view.read_range(offset, clamped)
    }

    pub fn cell_id_at(&self, offset: u64) -> Option<CellId> {
        self.pieces.resolve(offset)
    }

    /// Read a contiguous logical range into a Vec, walking pieces directly.
    ///
    /// Cheaper than `logical_bytes` for small reads (no `Piece` snapshot clone),
    /// and avoids the per-byte overhead of `byte_at` loops in format parse /
    /// detect. Tombstoned cells are rendered as `0x00`, matching the previous
    /// per-byte fallback used by format parsers. Returns `None` only when the
    /// starting offset is past EOF; short reads (offset + len > len) simply
    /// return fewer bytes than requested.
    pub fn read_logical_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let doc_len = self.len();
        if offset >= doc_len {
            return Ok(Vec::new());
        }
        let end = (offset + len as u64).min(doc_len);
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();

        // Resolve the piece containing `offset` so we can walk forward.
        // We snapshot piece metadata lazily (small allocation, only matters for
        // very piece-heavy docs).
        let pieces: Vec<Piece> = self.pieces.pieces().to_vec();
        let mut out = Vec::with_capacity((end - offset) as usize);
        let mut cursor = 0_u64;
        for piece in &pieces {
            if cursor >= end {
                break;
            }
            let piece_end = cursor + piece.len;
            if piece_end <= offset {
                cursor = piece_end;
                continue;
            }
            let overlap_start = offset.max(cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                cursor = piece_end;
                continue;
            }
            let source_start = piece.start + (overlap_start - cursor);
            let overlap_len = overlap_end - overlap_start;

            match piece.source {
                PieceSource::Original => {
                    let raw = self.raw_range(source_start, overlap_len as usize)?;
                    let read_len = raw.len();
                    if !has_tombstones && !has_replacements {
                        out.extend_from_slice(&raw);
                    } else {
                        let (need_ts, need_rep) = self.search_overlay_flags(
                            PieceSource::Original,
                            source_start,
                            read_len as u64,
                            has_tombstones,
                            has_replacements,
                        );
                        if !need_ts && !need_rep {
                            out.extend_from_slice(&raw);
                        } else {
                            for (i, &base) in raw.iter().enumerate() {
                                let id = CellId::Original(source_start + i as u64);
                                if need_ts && self.is_tombstone(id) {
                                    out.push(0);
                                    continue;
                                }
                                let byte = if need_rep {
                                    self.replacement_for(id).unwrap_or(base)
                                } else {
                                    base
                                };
                                out.push(byte);
                            }
                        }
                    }
                    // Short read: pad with 0 to keep the per-byte legacy behavior.
                    for _ in read_len..overlap_len as usize {
                        out.push(0);
                    }
                }
                PieceSource::Add => {
                    let slice = self.pieces.add_buffer_slice(source_start, overlap_len);
                    if !has_tombstones && !has_replacements {
                        out.extend_from_slice(slice);
                    } else {
                        let (need_ts, need_rep) = self.search_overlay_flags(
                            PieceSource::Add,
                            source_start,
                            overlap_len,
                            has_tombstones,
                            has_replacements,
                        );
                        if !need_ts && !need_rep {
                            out.extend_from_slice(slice);
                        } else {
                            for (i, &base) in slice.iter().enumerate() {
                                let id = CellId::Add(source_start + i as u64);
                                if need_ts && self.is_tombstone(id) {
                                    out.push(0);
                                    continue;
                                }
                                let byte = if need_rep {
                                    self.replacement_for(id).unwrap_or(base)
                                } else {
                                    base
                                };
                                out.push(byte);
                            }
                        }
                    }
                    for _ in slice.len()..overlap_len as usize {
                        out.push(0);
                    }
                }
            }
            cursor = piece_end;
        }
        Ok(out)
    }

    pub fn byte_at(&mut self, offset: u64) -> HxResult<ByteSlot> {
        let Some(id) = self.cell_id_at(offset) else {
            return Ok(ByteSlot::Empty);
        };
        if self.tombstones.contains(&id) {
            return Ok(ByteSlot::Deleted);
        }
        Ok(ByteSlot::Present(self.display_byte_for_id(id)?))
    }

    pub fn row_bytes(&mut self, offset: u64, width: usize) -> HxResult<Vec<ByteSlot>> {
        let doc_len = self.len();
        if width == 0 || offset >= doc_len {
            return Ok(vec![ByteSlot::Empty; width]);
        }

        let end = (offset + width as u64).min(doc_len);
        let actual = (end - offset) as usize;
        let mut out = Vec::with_capacity(width);
        let mut cursor = 0_u64;

        for piece in self.pieces.pieces() {
            if out.len() >= actual {
                break;
            }
            let piece_end = cursor + piece.len;
            if piece_end <= offset {
                cursor = piece_end;
                continue;
            }

            let overlap_start = offset.max(cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                cursor = piece_end;
                continue;
            }

            let source_start = piece.start + (overlap_start - cursor);
            let count = (overlap_end - overlap_start) as usize;

            match piece.source {
                PieceSource::Original => {
                    let raw = self.view.read_range(source_start, count)?;
                    for (idx, &base) in raw.iter().enumerate() {
                        let id = CellId::Original(source_start + idx as u64);
                        out.push(self.resolve_slot(id, base));
                    }
                    for _ in raw.len()..count {
                        out.push(ByteSlot::Empty);
                    }
                }
                PieceSource::Add => {
                    let slice = self.pieces.add_buffer_slice(source_start, count as u64);
                    for (idx, &base) in slice.iter().enumerate() {
                        let id = CellId::Add(source_start + idx as u64);
                        out.push(self.resolve_slot(id, base));
                    }
                    for _ in slice.len()..count {
                        out.push(ByteSlot::Empty);
                    }
                }
            }

            cursor = piece_end;
        }

        out.resize(width, ByteSlot::Empty);
        Ok(out)
    }

    /// Extract the actual bytes (skipping tombstones) in a display range.
    ///
    /// Walks the piece table once and reads in 64 KB chunks, using O(log n)
    /// range queries to skip clean chunks entirely — the same strategy as
    /// the save path.
    pub fn logical_bytes(&mut self, start: u64, end_inclusive: u64) -> HxResult<Vec<u8>> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(Vec::new());
        }

        let end = end_inclusive.min(len - 1) + 1;
        let pieces: Vec<Piece> = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let mut out = Vec::with_capacity((end - start) as usize);
        let mut cursor = 0_u64;

        for piece in &pieces {
            if cursor >= end {
                break;
            }
            let piece_end = cursor + piece.len;
            if piece_end <= start {
                cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                cursor = piece_end;
                continue;
            }

            let source_start = piece.start + (overlap_start - cursor);
            let overlap_len = overlap_end - overlap_start;

            match piece.source {
                PieceSource::Original => {
                    let mut remaining = overlap_len;
                    let mut file_off = source_start;
                    let mut cell_off = source_start;

                    while remaining > 0 {
                        let batch = (remaining as usize).min(LOGICAL_CHUNK);
                        let raw = self.raw_range(file_off, batch)?;
                        if raw.is_empty() {
                            break;
                        }
                        let read_len = raw.len() as u64;

                        let (need_ts, need_rep) = self.search_overlay_flags(
                            PieceSource::Original,
                            cell_off,
                            read_len,
                            has_tombstones,
                            has_replacements,
                        );

                        if !need_ts && !need_rep {
                            out.extend_from_slice(&raw);
                        } else {
                            for (i, &base) in raw.iter().enumerate() {
                                let id = CellId::Original(cell_off + i as u64);
                                if need_ts && self.is_tombstone(id) {
                                    continue;
                                }
                                let byte = if need_rep {
                                    self.replacement_for(id).unwrap_or(base)
                                } else {
                                    base
                                };
                                out.push(byte);
                            }
                        }

                        file_off += read_len;
                        cell_off += read_len;
                        remaining -= read_len;
                    }
                }
                PieceSource::Add => {
                    let (need_ts, need_rep) = self.search_overlay_flags(
                        PieceSource::Add,
                        source_start,
                        overlap_len,
                        has_tombstones,
                        has_replacements,
                    );

                    if !need_ts && !need_rep {
                        let slice = self.add_slice(source_start, overlap_len);
                        out.extend_from_slice(slice);
                    } else {
                        let slice = self.add_slice(source_start, overlap_len);
                        for (i, &base) in slice.iter().enumerate() {
                            let id = CellId::Add(source_start + i as u64);
                            if need_ts && self.is_tombstone(id) {
                                continue;
                            }
                            let byte = if need_rep {
                                self.replacement_for(id).unwrap_or(base)
                            } else {
                                base
                            };
                            out.push(byte);
                        }
                    }
                }
            }

            cursor = piece_end;
        }

        Ok(out)
    }

    /// Compute a hash over the logical bytes in a display range, streaming
    /// data through the hasher in 64 KB chunks without materializing the
    /// entire byte vector in memory.
    pub fn hash_logical_bytes(
        &mut self,
        start: u64,
        end_inclusive: u64,
        mut hasher: Box<dyn digest::DynDigest>,
    ) -> HxResult<(u64, Vec<u8>)> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok((0, Vec::new()));
        }

        let end = end_inclusive.min(len - 1) + 1;
        let pieces: Vec<Piece> = self.pieces_snapshot();
        let has_tombstones = self.has_tombstones();
        let has_replacements = self.has_replacements();
        let mut bytes_hashed: u64 = 0;
        let mut cursor = 0_u64;

        let mut chunk_buf = Vec::with_capacity(LOGICAL_CHUNK);

        for piece in &pieces {
            if cursor >= end {
                break;
            }
            let piece_end = cursor + piece.len;
            if piece_end <= start {
                cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                cursor = piece_end;
                continue;
            }

            let source_start = piece.start + (overlap_start - cursor);
            let overlap_len = overlap_end - overlap_start;

            match piece.source {
                PieceSource::Original => {
                    let mut remaining = overlap_len;
                    let mut file_off = source_start;
                    let mut cell_off = source_start;

                    while remaining > 0 {
                        let batch = (remaining as usize).min(LOGICAL_CHUNK);
                        let raw = self.raw_range(file_off, batch)?;
                        if raw.is_empty() {
                            break;
                        }
                        let read_len = raw.len() as u64;

                        let (need_ts, need_rep) = self.search_overlay_flags(
                            PieceSource::Original,
                            cell_off,
                            read_len,
                            has_tombstones,
                            has_replacements,
                        );

                        if !need_ts && !need_rep {
                            hasher.update(&raw);
                            bytes_hashed += read_len;
                        } else {
                            chunk_buf.clear();
                            for (i, &base) in raw.iter().enumerate() {
                                let id = CellId::Original(cell_off + i as u64);
                                if need_ts && self.is_tombstone(id) {
                                    continue;
                                }
                                let byte = if need_rep {
                                    self.replacement_for(id).unwrap_or(base)
                                } else {
                                    base
                                };
                                chunk_buf.push(byte);
                            }
                            hasher.update(&chunk_buf);
                            bytes_hashed += chunk_buf.len() as u64;
                        }

                        file_off += read_len;
                        cell_off += read_len;
                        remaining -= read_len;
                    }
                }
                PieceSource::Add => {
                    let (need_ts, need_rep) = self.search_overlay_flags(
                        PieceSource::Add,
                        source_start,
                        overlap_len,
                        has_tombstones,
                        has_replacements,
                    );

                    let slice = self.add_slice(source_start, overlap_len);

                    if !need_ts && !need_rep {
                        hasher.update(slice);
                        bytes_hashed += slice.len() as u64;
                    } else {
                        chunk_buf.clear();
                        for (i, &base) in slice.iter().enumerate() {
                            let id = CellId::Add(source_start + i as u64);
                            if need_ts && self.is_tombstone(id) {
                                continue;
                            }
                            let byte = if need_rep {
                                self.replacement_for(id).unwrap_or(base)
                            } else {
                                base
                            };
                            chunk_buf.push(byte);
                        }
                        hasher.update(&chunk_buf);
                        bytes_hashed += chunk_buf.len() as u64;
                    }
                }
            }

            cursor = piece_end;
        }

        let result = hasher.finalize();
        Ok((bytes_hashed, result.to_vec()))
    }
}
