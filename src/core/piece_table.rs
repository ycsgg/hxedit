//! Piece table — the core data structure for non-destructive editing.
//!
//! The piece table represents the document as an ordered sequence of *pieces*,
//! where each piece references a contiguous byte range in either the original
//! file (`Original`) or an append-only buffer of inserted bytes (`Add`).
//!
//! Insertions append data to the add-buffer and splice a new `Add` piece into
//! the sequence.  Real deletions (insert-mode backspace) remove pieces from the
//! sequence.  Neither operation ever mutates the original file.
//!
//! Normal-mode "tombstone" deletions are **not** handled here — they live in
//! `Document::tombstones` and only affect rendering / saving.

use std::cell::{Cell, RefCell};

/// Stable identity for a single byte in the document.
///
/// `CellId` survives insertions and deletions: an `Original(42)` always refers
/// to byte 42 of the on-disk file, regardless of how many bytes have been
/// inserted before it.  Tombstones and replacements are keyed by `CellId`.
///
/// The `Ord` implementation groups all `Original` ids before `Add` ids, which
/// lets `BTreeSet::range` queries efficiently test whether a piece's cell range
/// intersects the tombstone / replacement sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CellId {
    /// Byte at the given offset in the original file.
    Original(u64),
    /// Byte at the given offset in the add-buffer.
    Add(u64),
}

impl CellId {
    /// Construct a `CellId` from a [`PieceSource`] and an offset.
    pub fn from_source(source: PieceSource, offset: u64) -> Self {
        match source {
            PieceSource::Original => Self::Original(offset),
            PieceSource::Add => Self::Add(offset),
        }
    }
}

/// Which backing store a [`Piece`] draws its bytes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceSource {
    /// The read-only original file.
    Original,
    /// The append-only add-buffer (holds all inserted bytes).
    Add,
}

/// A contiguous run of bytes from a single source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub source: PieceSource,
    /// Byte offset within the source (file offset or add-buffer offset).
    pub start: u64,
    /// Number of bytes in this run.
    pub len: u64,
}

/// Piece table: an ordered list of [`Piece`]s plus an append-only add-buffer.
///
/// # Invariants
///
/// - `len` always equals the sum of all `piece.len` values.
/// - Adjacent pieces with the same source and contiguous offsets are merged
///   by [`coalesce`](Self::coalesce) after every mutation.
/// - The add-buffer is append-only; bytes are never removed from it (real
///   deletions remove pieces, not buffer content).
/// - `prefix_ends[i]` caches the cumulative display length through `pieces[i]`
///   when `prefix_dirty` is `false`. Used for O(log n) offset lookup in
///   [`find_piece`](Self::find_piece). Mutations set `prefix_dirty = true`
///   and the cache is lazily rebuilt on the next read.
#[derive(Debug, Clone)]
pub struct PieceTable {
    /// Length of the original file (used by [`is_identity`](Self::is_identity)).
    original_len: u64,
    /// Append-only buffer holding every byte ever inserted.
    add_buffer: Vec<u8>,
    /// Ordered sequence of pieces describing the current document content.
    pieces: Vec<Piece>,
    /// Cached total display length (sum of all piece lengths).
    len: u64,
    /// Cumulative display length at the end of each piece (lazy).
    prefix_ends: RefCell<Vec<u64>>,
    /// True when `prefix_ends` is stale and must be rebuilt before use.
    prefix_dirty: Cell<bool>,
}

impl PieceTable {
    /// Create a new piece table for a file of `original_len` bytes.
    ///
    /// Starts with a single `Original` piece spanning the entire file
    /// (or an empty piece list for a zero-length file).
    pub fn new(original_len: u64) -> Self {
        let pieces = if original_len == 0 {
            Vec::new()
        } else {
            vec![Piece {
                source: PieceSource::Original,
                start: 0,
                len: original_len,
            }]
        };
        Self {
            original_len,
            add_buffer: Vec::new(),
            pieces,
            len: original_len,
            prefix_ends: RefCell::new(Vec::new()),
            prefix_dirty: Cell::new(true),
        }
    }

    /// Total display length (includes tombstoned slots — tombstones are
    /// tracked externally by `Document`).
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns `true` when no edits have been made: the piece list is a
    /// single `Original` piece covering the entire file.
    pub fn is_identity(&self) -> bool {
        if self.len != self.original_len {
            return false;
        }
        if self.original_len == 0 {
            return self.pieces.is_empty();
        }
        self.pieces.len() == 1
            && self.pieces[0]
                == Piece {
                    source: PieceSource::Original,
                    start: 0,
                    len: self.original_len,
                }
    }

    /// Current length of the add-buffer (used to compute `CellId::Add`
    /// offsets for newly inserted bytes).
    pub fn add_len(&self) -> u64 {
        self.add_buffer.len() as u64
    }

    /// Read a single byte from the add-buffer by offset.
    pub fn add_byte(&self, offset: u64) -> Option<u8> {
        self.add_buffer.get(offset as usize).copied()
    }

    /// Borrow the piece list (used by the save path to walk pieces directly).
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// Borrow a slice of the add-buffer.  Used by the save path to write
    /// `Add` pieces in bulk without per-byte allocation.
    pub fn add_buffer_slice(&self, start: u64, len: u64) -> &[u8] {
        let end = (start + len) as usize;
        let end = end.min(self.add_buffer.len());
        let start = start as usize;
        if start >= end {
            &[]
        } else {
            &self.add_buffer[start..end]
        }
    }

    /// Ensure the prefix-sum cache is up to date. O(pieces) rebuild when
    /// dirty, O(1) otherwise. Called lazily from read paths.
    fn ensure_prefix(&self) {
        if !self.prefix_dirty.get() {
            return;
        }
        let mut ends = self.prefix_ends.borrow_mut();
        ends.clear();
        ends.reserve(self.pieces.len());
        let mut cursor = 0_u64;
        for piece in &self.pieces {
            cursor += piece.len;
            ends.push(cursor);
        }
        self.prefix_dirty.set(false);
    }

    /// Locate the piece containing `display_offset`. Returns `(idx, cursor)`
    /// where `cursor` is the display offset of the start of piece `idx`.
    /// Returns `None` if the offset is past EOF.
    fn find_piece(&self, display_offset: u64) -> Option<(usize, u64)> {
        if display_offset >= self.len || self.pieces.is_empty() {
            return None;
        }
        self.ensure_prefix();
        let ends = self.prefix_ends.borrow();
        // First index whose end > display_offset.
        let idx = ends.partition_point(|end| *end <= display_offset);
        if idx >= self.pieces.len() {
            return None;
        }
        let cursor = if idx == 0 { 0 } else { ends[idx - 1] };
        Some((idx, cursor))
    }

    /// Mark the prefix-sum cache stale. Called by every mutation.
    fn invalidate_prefix(&mut self) {
        self.prefix_dirty.set(true);
    }

    /// Map a display offset to its [`CellId`].
    ///
    /// Uses the (lazily built) prefix-sum cache + binary search — O(log n) in
    /// the number of pieces. Returns `None` if the offset is past the end.
    pub fn resolve(&self, display_offset: u64) -> Option<CellId> {
        if display_offset >= self.len {
            return None;
        }
        let (idx, cursor) = self.find_piece(display_offset)?;
        let piece = self.pieces[idx];
        let source_offset = piece.start + (display_offset - cursor);
        Some(match piece.source {
            PieceSource::Original => CellId::Original(source_offset),
            PieceSource::Add => CellId::Add(source_offset),
        })
    }

    /// Collect `CellId`s for a contiguous display range.
    ///
    /// Used by `delete_range_real` to snapshot which cells are about to be
    /// removed (so undo can re-insert them), and by the old `visible_len`
    /// implementation (now replaced by O(1) arithmetic).
    pub fn cell_ids_range(&self, display_offset: u64, len: u64) -> Vec<CellId> {
        if len == 0 || display_offset >= self.len {
            return Vec::new();
        }

        let end = display_offset.saturating_add(len).min(self.len);
        let mut out = Vec::with_capacity((end - display_offset) as usize);
        let Some((start_idx, mut cursor)) = self.find_piece(display_offset) else {
            return out;
        };

        for piece in &self.pieces[start_idx..] {
            if cursor >= end {
                break;
            }

            let piece_end = cursor + piece.len;
            let overlap_start = display_offset.max(cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - cursor);
                for idx in 0..(overlap_end - overlap_start) {
                    out.push(match piece.source {
                        PieceSource::Original => CellId::Original(source_start + idx),
                        PieceSource::Add => CellId::Add(source_start + idx),
                    });
                }
            }

            cursor = piece_end;
        }

        out
    }

    /// Insert new bytes at `display_offset`.
    ///
    /// Appends `bytes` to the add-buffer, splits the piece list at the
    /// insertion point, and inserts a new `Add` piece.  Subsequent display
    /// offsets shift right by `bytes.len()`.
    pub fn insert_bytes(&mut self, display_offset: u64, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        let add_start = self.add_buffer.len() as u64;
        self.add_buffer.extend_from_slice(bytes);

        let piece = Piece {
            source: PieceSource::Add,
            start: add_start,
            len: bytes.len() as u64,
        };
        let insert_idx = self.split_piece_at(display_offset.min(self.len));
        self.pieces.insert(insert_idx, piece);
        self.len += piece.len;
        self.coalesce();
        self.invalidate_prefix();
    }

    /// Re-insert previously removed cells (used by undo of real-delete).
    ///
    /// Converts the `CellId` list back into pieces and splices them in at
    /// `display_offset`.  This restores the exact bytes that were removed
    /// by a prior `delete_range_real`.
    pub fn insert_existing_cells(&mut self, display_offset: u64, cells: &[CellId]) {
        if cells.is_empty() {
            return;
        }

        let insert_idx = self.split_piece_at(display_offset.min(self.len));
        let new_pieces = cells_to_pieces(cells);
        let inserted_len = cells.len() as u64;
        self.pieces.splice(insert_idx..insert_idx, new_pieces);
        self.len += inserted_len;
        self.coalesce();
        self.invalidate_prefix();
    }

    /// Remove bytes from the display stream (insert-mode backspace).
    ///
    /// Returns the `CellId`s of the removed bytes so the caller can push
    /// an undo step that re-inserts them via `insert_existing_cells`.
    ///
    /// This is a *real* deletion — subsequent display offsets shift left
    /// immediately.  Normal-mode deletions use tombstones instead.
    pub fn delete_range_real(&mut self, display_offset: u64, len: u64) -> Vec<CellId> {
        if len == 0 || display_offset >= self.len {
            return Vec::new();
        }

        let end = display_offset.saturating_add(len).min(self.len);
        let removed = self.cell_ids_range(display_offset, end - display_offset);
        if removed.is_empty() {
            return removed;
        }

        let start_idx = self.split_piece_at(display_offset);
        let end_idx = self.split_piece_at(end);
        self.pieces.drain(start_idx..end_idx);
        self.len -= end - display_offset;
        self.coalesce();
        self.invalidate_prefix();
        removed
    }

    /// Split the piece that spans `display_offset` into two halves.
    ///
    /// Returns the index where a new piece should be inserted. If the
    /// offset falls on a piece boundary, no split is needed and the
    /// existing boundary index is returned. Uses binary search on the
    /// prefix-sum cache to find the piece in O(log n).
    fn split_piece_at(&mut self, display_offset: u64) -> usize {
        if display_offset == 0 {
            return 0;
        }
        if display_offset >= self.len {
            return self.pieces.len();
        }

        let Some((idx, cursor)) = self.find_piece(display_offset) else {
            return self.pieces.len();
        };
        let piece = self.pieces[idx];
        let piece_end = cursor + piece.len;

        if display_offset == cursor {
            return idx;
        }
        if display_offset == piece_end {
            return idx + 1;
        }

        let left_len = display_offset - cursor;
        let right_len = piece.len - left_len;
        let right = Piece {
            source: piece.source,
            start: piece.start + left_len,
            len: right_len,
        };
        self.pieces[idx].len = left_len;
        self.pieces.insert(idx + 1, right);
        self.prefix_dirty.set(true);
        idx + 1
    }

    /// Merge adjacent pieces that reference contiguous ranges in the same
    /// source.  Called after every mutation to keep the piece list compact.
    fn coalesce(&mut self) {
        if self.pieces.is_empty() {
            return;
        }

        let mut merged: Vec<Piece> = Vec::with_capacity(self.pieces.len());
        for piece in self.pieces.drain(..) {
            if piece.len == 0 {
                continue;
            }
            if let Some(last) = merged.last_mut() {
                if last.source == piece.source && last.start + last.len == piece.start {
                    last.len += piece.len;
                    continue;
                }
            }
            merged.push(piece);
        }
        self.pieces = merged;
    }
}

/// Convert a list of `CellId`s back into a minimal list of pieces,
/// merging consecutive ids from the same source.
fn cells_to_pieces(cells: &[CellId]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for &cell in cells {
        let (source, start) = match cell {
            CellId::Original(offset) => (PieceSource::Original, offset),
            CellId::Add(offset) => (PieceSource::Add, offset),
        };

        if let Some(last) = out.last_mut() {
            if last.source == source && last.start + last.len == start {
                last.len += 1;
                continue;
            }
        }

        out.push(Piece {
            source,
            start,
            len: 1,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{CellId, PieceTable};

    /// Inserting two bytes in the middle of a 4-byte original file produces
    /// three pieces: Original[0..2], Add[0..2], Original[2..4].
    #[test]
    fn inserts_into_middle() {
        let mut table = PieceTable::new(4);
        table.insert_bytes(2, &[0xaa, 0xbb]);

        assert_eq!(table.len(), 6);
        assert_eq!(
            table.cell_ids_range(0, 6),
            vec![
                CellId::Original(0),
                CellId::Original(1),
                CellId::Add(0),
                CellId::Add(1),
                CellId::Original(2),
                CellId::Original(3),
            ]
        );
    }

    /// Real-delete removes pieces from the table; re-inserting the saved
    /// `CellId`s restores the original layout.
    #[test]
    fn real_delete_and_restore_round_trip() {
        let mut table = PieceTable::new(4);
        // After insert: O(0) | A(0) A(1) | O(1) O(2) O(3)
        table.insert_bytes(1, &[0xaa, 0xbb]);
        // Delete 3 bytes starting at display offset 1: removes A(0), A(1), O(1)
        let removed = table.delete_range_real(1, 3);

        assert_eq!(
            table.cell_ids_range(0, table.len()),
            vec![
                CellId::Original(0),
                CellId::Original(2),
                CellId::Original(3)
            ]
        );

        // Re-insert the removed cells at offset 1 to restore the original state
        table.insert_existing_cells(1, &removed);
        assert_eq!(
            table.cell_ids_range(0, table.len()),
            vec![
                CellId::Original(0),
                CellId::Add(0),
                CellId::Add(1),
                CellId::Original(1),
                CellId::Original(2),
                CellId::Original(3),
            ]
        );
    }
}
