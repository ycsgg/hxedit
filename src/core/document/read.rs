use crate::core::document::{ByteSlot, Document};
use crate::core::piece_table::{CellId, PieceSource};
use crate::error::HxResult;

impl Document {
    /// Read raw bytes from the original file (bypasses piece table / overlays).
    /// Used by the save path for bulk reads.
    pub fn raw_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if offset >= self.original_len {
            return Ok(Vec::new());
        }
        let clamped = len.min((self.original_len - offset) as usize);
        self.view.read_range(offset, clamped)
    }

    /// Resolve a display offset to its stable [`CellId`].
    pub fn cell_id_at(&self, offset: u64) -> Option<CellId> {
        self.pieces.resolve(offset)
    }

    /// Read a single display slot: `Present(byte)`, `Deleted`, or `Empty`.
    pub fn byte_at(&mut self, offset: u64) -> HxResult<ByteSlot> {
        let Some(id) = self.cell_id_at(offset) else {
            return Ok(ByteSlot::Empty);
        };
        if self.tombstones.contains(&id) {
            return Ok(ByteSlot::Deleted);
        }
        Ok(ByteSlot::Present(self.display_byte_for_id(id)?))
    }

    /// Read a row of display slots starting at `offset`.
    ///
    /// Walks the piece table once to resolve all bytes in the range,
    /// avoiding repeated O(pieces) lookups per byte.
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
    /// Used by copy to get the selection content.
    pub fn logical_bytes(&mut self, start: u64, end_inclusive: u64) -> HxResult<Vec<u8>> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(Vec::new());
        }

        let mut out = Vec::with_capacity((end_inclusive - start + 1) as usize);
        for offset in start..=end_inclusive.min(len - 1) {
            if let ByteSlot::Present(byte) = self.byte_at(offset)? {
                out.push(byte);
            }
        }
        Ok(out)
    }
}
