use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod edit;
mod read;
mod search;

use crate::config::Config;
use crate::core::file_view::FileView;
use crate::core::page_cache::CacheStats;
use crate::core::piece_table::{CellId, Piece, PieceSource, PieceTable};
use crate::core::save;
use crate::error::{HxError, HxResult};

const SEARCH_CHUNK: usize = 64 * 1024;

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

    pub fn is_empty(&self) -> bool {
        self.pieces.len() == 0
    }

    /// Number of bytes that would be written on save (display len minus tombstones).
    pub fn visible_len(&self) -> u64 {
        self.pieces
            .len()
            .saturating_sub(self.tombstones.len() as u64)
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

    /// Save the document to disk. Walks the piece list in bulk, skipping
    /// tombstones and applying replacements. After saving, resets all edit
    /// state and reloads the file.
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
        if self.is_empty() {
            return Ok(0);
        }
        if offset >= self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok(offset)
    }

    /// Cheap snapshot of the piece list (small structs, no data).
    pub fn pieces_snapshot(&self) -> Vec<Piece> {
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

impl Document {
    /// Resolve a single cell to its display slot given the base byte.
    fn resolve_slot(&self, id: CellId, base: u8) -> ByteSlot {
        if self.tombstones.contains(&id) {
            return ByteSlot::Deleted;
        }
        let byte = self.replacements.get(&id).copied().unwrap_or(base);
        ByteSlot::Present(byte)
    }

    /// Resolve a display offset to (CellId, current display byte) for editing.
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

    /// Set the display value for a cell. If the new value equals the base
    /// byte, the replacement entry is removed.
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
            CellId::Add(offset) => self
                .pieces
                .add_byte(offset)
                .ok_or(HxError::OffsetOutOfRange),
        }
    }

    /// Read a single byte from the original file via the page cache.
    fn raw_byte(&mut self, offset: u64) -> HxResult<u8> {
        let raw = self.view.read_range(offset, 1)?;
        raw.first().copied().ok_or(HxError::OffsetOutOfRange)
    }

    /// Read a chunk of bytes from the given source (Original file or Add buffer).
    fn read_chunk(&mut self, source: PieceSource, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        match source {
            PieceSource::Original => self.raw_range(offset, len),
            PieceSource::Add => Ok(self.add_slice(offset, len as u64).to_vec()),
        }
    }

    fn search_overlay_flags(
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
