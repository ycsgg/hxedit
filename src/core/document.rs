use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::core::file_view::FileView;
use crate::core::page_cache::CacheStats;
use crate::core::piece_table::{CellId, PieceTable};
use crate::core::save;
use crate::error::{HxError, HxResult};
use crate::mode::NibblePhase;

/// What the renderer sees at a given display offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteSlot {
    /// A visible byte with the given value (after applying replacements).
    Present(u8),
    /// A tombstone-deleted byte — still occupies a display slot but is
    /// rendered as "XX" and skipped on save.
    Deleted,
    /// Past the end of the document.
    Empty,
}

/// Backing document model for the editor.
///
/// Composes three layers:
///
/// 1. **PieceTable** — tracks insertions and real deletions (insert-mode
///    backspace).  These immediately shift subsequent display offsets.
/// 2. **Tombstones** (`BTreeSet<CellId>`) — normal/visual-mode deletions.
///    The cell still occupies its display slot (shown as `Deleted`) but is
///    skipped on save.
/// 3. **Replacements** (`BTreeMap<CellId, u8>`) — in-place byte edits
///    (edit-mode nibble changes).  The replacement value overrides the base
///    byte from the file or add-buffer.
///
/// All external interfaces use *display offsets* derived from the piece table.
#[derive(Debug)]
pub struct Document {
    path: PathBuf,
    readonly: bool,
    page_size: usize,
    cache_pages: usize,
    original_len: u64,
    view: FileView,
    pieces: PieceTable,
    tombstones: BTreeSet<CellId>,
    replacements: BTreeMap<CellId, u8>,
}

impl Document {
    /// Open a document from disk with the given configuration.
    pub fn open(path: &Path, config: &Config) -> HxResult<Self> {
        let view = FileView::open(path, config.readonly, config.page_size, config.cache_pages)?;
        let original_len = view.len();
        Ok(Self {
            path: path.to_path_buf(),
            readonly: config.readonly,
            page_size: config.page_size,
            cache_pages: config.cache_pages,
            original_len,
            view,
            pieces: PieceTable::new(original_len),
            tombstones: BTreeSet::new(),
            replacements: BTreeMap::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn original_len(&self) -> u64 {
        self.original_len
    }

    /// Total display length including tombstoned slots.
    pub fn len(&self) -> u64 {
        self.pieces.len()
    }

    /// Number of bytes that would be written on save (display len minus tombstones).
    ///
    /// O(1) — simply subtracts the tombstone count from the piece table length.
    pub fn visible_len(&self) -> u64 {
        self.pieces.len().saturating_sub(self.tombstones.len() as u64)
    }

    /// True when any edits (inserts, deletions, replacements) have been made
    /// since the last save.
    pub fn is_dirty(&self) -> bool {
        !self.pieces.is_identity() || !self.tombstones.is_empty() || !self.replacements.is_empty()
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    pub fn io_stats(&self) -> CacheStats {
        self.view.cache_stats()
    }

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

    /// Get the current replacement value for a cell (used by undo to snapshot
    /// the "before" state).
    pub fn replacement_state(&self, id: CellId) -> Option<u8> {
        self.replacements.get(&id).copied()
    }

    /// Restore a replacement to its previous state (used by undo).
    pub fn restore_replacement(&mut self, id: CellId, previous: Option<u8>) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        match previous {
            Some(value) => {
                self.replacements.insert(id, value);
            }
            None => {
                self.replacements.remove(&id);
            }
        }
        Ok(())
    }

    /// Tombstone-delete a byte (normal/visual mode).  The cell keeps its
    /// display slot but renders as `Deleted` and is skipped on save.
    pub fn mark_tombstone(&mut self, offset: u64) -> HxResult<Option<CellId>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let id = self.cell_id_at(offset).ok_or(HxError::OffsetOutOfRange)?;
        Ok(self.tombstones.insert(id).then_some(id))
    }

    /// Remove tombstones (used by undo of tombstone-delete).
    pub fn clear_tombstones(&mut self, ids: &[CellId]) {
        for id in ids {
            self.tombstones.remove(id);
        }
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
    pub fn row_bytes(&mut self, offset: u64, width: usize) -> HxResult<Vec<ByteSlot>> {
        let mut out = Vec::with_capacity(width);
        for idx in 0..width {
            out.push(self.byte_at(offset + idx as u64)?);
        }
        Ok(out)
    }

    /// Replace a single nibble (high or low) of the byte at `offset`.
    /// Used by edit-mode hex input.  If `offset == len`, inserts a new byte
    /// (only valid for the high nibble).
    pub fn replace_nibble(
        &mut self,
        offset: u64,
        phase: NibblePhase,
        nibble: u8,
    ) -> HxResult<CellId> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset == self.len() {
            if matches!(phase, NibblePhase::High) {
                return self.insert_byte(offset, nibble << 4);
            }
            return Err(HxError::OffsetOutOfRange);
        }

        let (id, current) = self.display_byte_for_edit(offset)?;
        let updated = match phase {
            NibblePhase::High => (nibble << 4) | (current & 0x0f),
            NibblePhase::Low => (current & 0xf0) | nibble,
        };
        self.set_display_byte_by_id(id, updated)?;
        Ok(id)
    }

    /// Replace the entire byte at `offset` with `value`.
    /// Used by insert-mode to fill in the low nibble of a pending byte.
    pub fn replace_display_byte(&mut self, offset: u64, value: u8) -> HxResult<CellId> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let (id, _) = self.display_byte_for_edit(offset)?;
        self.set_display_byte_by_id(id, value)?;
        Ok(id)
    }

    /// Set a byte: replace if within bounds, insert if at EOF.
    pub fn set_byte(&mut self, offset: u64, value: u8) -> HxResult<()> {
        if offset == self.len() {
            self.insert_byte(offset, value)?;
            return Ok(());
        }
        self.replace_display_byte(offset, value)?;
        Ok(())
    }

    /// Insert a single byte at `offset`.  Subsequent display offsets shift right.
    pub fn insert_byte(&mut self, offset: u64, value: u8) -> HxResult<CellId> {
        let inserted = self.insert_bytes(offset, &[value])?;
        inserted.first().copied().ok_or(HxError::OffsetOutOfRange)
    }

    /// Insert multiple bytes at `offset`.  Returns the `CellId`s of the new
    /// bytes (used by paste to build an undo step).
    pub fn insert_bytes(&mut self, offset: u64, bytes: &[u8]) -> HxResult<Vec<CellId>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        if bytes.is_empty() {
            return Ok(Vec::new());
        }

        let add_start = self.pieces.add_len();
        self.pieces.insert_bytes(offset, bytes);
        Ok((0..bytes.len())
            .map(|idx| CellId::Add(add_start + idx as u64))
            .collect())
    }

    /// Tombstone-delete a byte (convenience wrapper over `mark_tombstone`).
    pub fn delete_byte(&mut self, offset: u64) -> HxResult<Option<CellId>> {
        self.mark_tombstone(offset)
    }

    /// Real-delete bytes from the piece table (insert-mode backspace).
    /// Returns the removed `CellId`s so undo can re-insert them.
    /// Subsequent display offsets shift left immediately.
    pub fn delete_range_real(&mut self, offset: u64, len: u64) -> HxResult<Vec<CellId>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if len == 0 {
            return Ok(Vec::new());
        }
        if offset >= self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok(self.pieces.delete_range_real(offset, len))
    }

    /// Re-insert previously removed cells (undo of real-delete).
    pub fn restore_real_delete(&mut self, offset: u64, cells: &[CellId]) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        self.pieces.insert_existing_cells(offset, cells);
        Ok(())
    }

    /// Save the document to disk.  Walks the piece list in bulk, skipping
    /// tombstones and applying replacements.  After saving, resets all edit
    /// state (piece table, tombstones, replacements) and reloads the file.
    ///
    /// Returns the saved path and a [`SaveProfile`](save::SaveProfile) with
    /// timing / throughput information for the status bar.
    pub fn save(&mut self, path: Option<PathBuf>) -> HxResult<(PathBuf, save::SaveProfile)> {
        let target = path.unwrap_or_else(|| self.path.clone());
        if target == self.path && self.readonly {
            return Err(HxError::ReadOnly);
        }

        let profile = save::save_rewrite(self, &target)?;

        self.path = target.clone();
        self.view
            .reload(&target, self.readonly, self.page_size, self.cache_pages)?;
        self.original_len = self.view.len();
        self.pieces = PieceTable::new(self.original_len);
        self.tombstones.clear();
        self.replacements.clear();
        Ok((target, profile))
    }

    /// Validate and return a display offset for `:goto`.
    pub fn goto(&self, offset: u64) -> HxResult<u64> {
        if self.len() == 0 {
            return Ok(0);
        }
        if offset >= self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok(offset)
    }

    /// Search forward through the display stream.  Tombstoned bytes break
    /// matches (they are treated as gaps).  Inserted bytes participate normally.
    pub fn search_forward(&mut self, start: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        if pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        if start >= self.len() {
            return Ok(None);
        }

        let mut offsets = VecDeque::with_capacity(pattern.len());
        let mut bytes = VecDeque::with_capacity(pattern.len());

        for offset in start..self.len() {
            match self.byte_at(offset)? {
                ByteSlot::Present(byte) => {
                    offsets.push_back(offset);
                    bytes.push_back(byte);
                    if bytes.len() > pattern.len() {
                        offsets.pop_front();
                        bytes.pop_front();
                    }
                    if bytes.len() == pattern.len()
                        && bytes.iter().copied().eq(pattern.iter().copied())
                    {
                        return Ok(offsets.front().copied());
                    }
                }
                ByteSlot::Deleted | ByteSlot::Empty => {
                    offsets.clear();
                    bytes.clear();
                }
            }
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

        let mut offsets = VecDeque::with_capacity(pattern.len());
        let mut bytes = VecDeque::with_capacity(pattern.len());

        for offset in (0..end).rev() {
            match self.byte_at(offset)? {
                ByteSlot::Present(byte) => {
                    offsets.push_front(offset);
                    bytes.push_front(byte);
                    if bytes.len() > pattern.len() {
                        offsets.pop_back();
                        bytes.pop_back();
                    }
                    if bytes.len() == pattern.len()
                        && bytes.iter().copied().eq(pattern.iter().copied())
                    {
                        return Ok(offsets.front().copied());
                    }
                }
                ByteSlot::Deleted | ByteSlot::Empty => {
                    offsets.clear();
                    bytes.clear();
                }
            }
        }

        Ok(None)
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

    /// Resolve a display offset to (CellId, current display byte) for editing.
    /// Returns an error if the cell is tombstoned or out of range.
    fn display_byte_for_edit(&mut self, offset: u64) -> HxResult<(CellId, u8)> {
        let id = self.cell_id_at(offset).ok_or(HxError::OffsetOutOfRange)?;
        if self.tombstones.contains(&id) {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok((id, self.display_byte_for_id(id)?))
    }

    /// Get the display value for a cell: replacement if present, else base byte.
    fn display_byte_for_id(&mut self, id: CellId) -> HxResult<u8> {
        if let Some(value) = self.replacements.get(&id).copied() {
            return Ok(value);
        }
        self.base_byte(id)
    }

    /// Set the display value for a cell.  If the new value equals the base
    /// byte, the replacement entry is removed (no-op edit).
    fn set_display_byte_by_id(&mut self, id: CellId, value: u8) -> HxResult<()> {
        let base = self.base_byte(id)?;
        if value == base {
            self.replacements.remove(&id);
        } else {
            self.replacements.insert(id, value);
        }
        Ok(())
    }

    /// Read the base (unmodified) byte for a cell from the file or add-buffer.
    fn base_byte(&mut self, id: CellId) -> HxResult<u8> {
        match id {
            CellId::Original(offset) => self.raw_byte(offset),
            CellId::Add(offset) => self.pieces.add_byte(offset).ok_or(HxError::OffsetOutOfRange),
        }
    }

    /// Read a single byte from the original file via the page cache.
    fn raw_byte(&mut self, offset: u64) -> HxResult<u8> {
        let raw = self.view.read_range(offset, 1)?;
        raw.first().copied().ok_or(HxError::OffsetOutOfRange)
    }

    // ── helpers used by save::write_pieces ──────────────────────────

    /// Cheap snapshot of the piece list (small structs, no data).
    pub fn pieces_snapshot(&self) -> Vec<crate::core::piece_table::Piece> {
        self.pieces.pieces().to_vec()
    }

    /// Check whether a cell is tombstoned (O(log n) BTreeSet lookup).
    pub fn is_tombstone(&self, id: CellId) -> bool {
        self.tombstones.contains(&id)
    }

    /// True when any tombstones exist.
    pub fn has_tombstones(&self) -> bool {
        !self.tombstones.is_empty()
    }

    /// True when any replacements exist.
    pub fn has_replacements(&self) -> bool {
        !self.replacements.is_empty()
    }

    /// Check if any tombstone falls within a CellId range (inclusive).
    /// Uses BTreeSet range queries for O(log n) instead of scanning every byte.
    pub fn has_tombstone_in_range(&self, lo: CellId, hi: CellId) -> bool {
        use std::ops::Bound;
        self.tombstones
            .range((Bound::Included(lo), Bound::Included(hi)))
            .next()
            .is_some()
    }

    /// Check if any replacement falls within a CellId range (inclusive).
    pub fn has_replacement_in_range(&self, lo: CellId, hi: CellId) -> bool {
        use std::ops::Bound;
        self.replacements
            .range((Bound::Included(lo), Bound::Included(hi)))
            .next()
            .is_some()
    }

    /// Return the replacement value for a cell, if any.
    pub fn replacement_for(&self, id: CellId) -> Option<u8> {
        self.replacements.get(&id).copied()
    }

    /// Borrow a slice of the add-buffer.
    pub fn add_slice(&self, start: u64, len: u64) -> &[u8] {
        self.pieces.add_buffer_slice(start, len)
    }
}
