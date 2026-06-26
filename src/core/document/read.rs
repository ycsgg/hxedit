use crate::core::document::{ByteSlot, Document};
use crate::core::piece_table::CellId;
use crate::error::HxResult;

use super::walk::WalkControl;

const LOGICAL_CHUNK: usize = 64 * 1024;
const REMOTE_STREAM_CHUNK: usize = 32 * 1024 * 1024;

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
    /// Shares the central visible-cell walker and avoids the per-byte overhead
    /// of `byte_at` loops in format parse / detect. Tombstoned cells are
    /// rendered as `0x00`, matching the previous per-byte fallback used by
    /// format parsers. Starting past EOF returns an empty Vec; short reads
    /// (offset + len > len) return fewer bytes than requested.
    pub fn read_logical_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let doc_len = self.len();
        if offset >= doc_len {
            return Ok(Vec::new());
        }
        let end_inclusive = (offset + len as u64).min(doc_len).saturating_sub(1);
        let mut out = Vec::with_capacity((end_inclusive - offset + 1) as usize);
        self.walk_visible_cells(
            offset,
            end_inclusive,
            self.max_contiguous_read_len(),
            |_, chunk| {
                for cell in chunk.cells {
                    out.push(if cell.deleted { 0 } else { cell.byte });
                }
                Ok(WalkControl::Continue)
            },
        )?;
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
        self.walk_visible_cells(offset, end - 1, width.max(1), |_, chunk| {
            for cell in chunk.cells {
                out.push(if cell.deleted {
                    ByteSlot::Deleted
                } else {
                    ByteSlot::Present(cell.byte)
                });
            }
            Ok(WalkControl::Continue)
        })?;

        out.resize(width, ByteSlot::Empty);
        debug_assert!(out.len() >= actual);
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

        let end = end_inclusive.min(len - 1);
        if let Some((start, end)) = self.clean_remote_original_range(start, end) {
            return self.read_clean_remote_original_range(start, end);
        }

        let mut out = Vec::with_capacity((end - start + 1) as usize);
        self.walk_logical_chunks(start, end, LOGICAL_CHUNK, |chunk| {
            out.extend_from_slice(chunk.bytes);
            Ok(WalkControl::Continue)
        })?;

        Ok(out)
    }

    /// Count logical bytes in a display range without materializing them.
    pub fn logical_byte_count(&mut self, start: u64, end_inclusive: u64) -> HxResult<u64> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(0);
        }

        let mut count = 0_u64;
        self.walk_logical_chunks(start, end_inclusive, LOGICAL_CHUNK, |chunk| {
            count += chunk.bytes.len() as u64;
            Ok(WalkControl::Continue)
        })?;

        Ok(count)
    }

    /// Walk the logical bytes in a display range, invoking `sink` with each
    /// 64 KB chunk (tombstones skipped, replacements applied) without
    /// materializing the entire byte vector in memory.
    ///
    /// This is the streaming primitive behind binary `:export` and
    /// logical-byte transforms, mirroring [`Document::hash_logical_bytes`]:
    /// clean original
    /// chunks are forwarded straight from the page-cache read buffer, while
    /// chunks overlapping tombstones/replacements are condensed into a scratch
    /// buffer first. Returns the total number of logical bytes visited.
    pub fn for_each_logical_chunk(
        &mut self,
        start: u64,
        end_inclusive: u64,
        mut sink: impl FnMut(&[u8]) -> HxResult<()>,
    ) -> HxResult<u64> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(0);
        }

        if let Some((start, end)) = self.clean_remote_original_range(start, end_inclusive) {
            return self.for_each_clean_remote_original_chunk(start, end, sink);
        }

        let mut visited: u64 = 0;
        self.walk_logical_chunks(start, end_inclusive, LOGICAL_CHUNK, |chunk| {
            if !chunk.bytes.is_empty() {
                sink(chunk.bytes)?;
                visited += chunk.bytes.len() as u64;
            }
            Ok(WalkControl::Continue)
        })?;

        Ok(visited)
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

        let bytes_hashed = self.for_each_logical_chunk(start, end_inclusive, |bytes| {
            if !bytes.is_empty() {
                hasher.update(bytes);
            }
            Ok(())
        })?;

        let result = hasher.finalize();
        Ok((bytes_hashed, result.to_vec()))
    }

    pub(crate) fn clean_remote_original_range(
        &self,
        start: u64,
        end_inclusive: u64,
    ) -> Option<(u64, u64)> {
        if !self.view.is_remote() || self.is_dirty() {
            return None;
        }
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return None;
        }
        Some((start, end_inclusive.min(len - 1)))
    }

    pub(crate) fn read_clean_remote_original_range(
        &mut self,
        start: u64,
        end: u64,
    ) -> HxResult<Vec<u8>> {
        let len = end.saturating_sub(start).saturating_add(1);
        let read_len = len as usize;
        self.view
            .read_remote_direct(start, read_len)?
            .ok_or_else(|| unreachable!("clean remote fast path checked remote storage"))
    }

    pub(crate) fn for_each_clean_remote_original_chunk(
        &mut self,
        start: u64,
        end: u64,
        mut sink: impl FnMut(&[u8]) -> HxResult<()>,
    ) -> HxResult<u64> {
        let mut offset = start;
        let mut visited = 0_u64;
        while offset <= end {
            let remaining = end.saturating_sub(offset).saturating_add(1);
            let read_len = if remaining > REMOTE_STREAM_CHUNK as u64 {
                REMOTE_STREAM_CHUNK
            } else {
                remaining as usize
            };
            let chunk = self
                .view
                .read_remote_direct(offset, read_len)?
                .expect("clean remote fast path checked remote storage");
            if chunk.is_empty() {
                break;
            }
            let read = chunk.len() as u64;
            sink(&chunk)?;
            visited += read;
            offset = offset.saturating_add(read);
            if chunk.len() < read_len {
                break;
            }
        }
        Ok(visited)
    }
}
