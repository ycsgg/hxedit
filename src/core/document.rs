use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::core::file_view::FileView;
use crate::core::page_cache::CacheStats;
use crate::core::patch::{PatchSet, PatchState};
use crate::core::save;
use crate::core::search;
use crate::error::{HxError, HxResult};
use crate::mode::NibblePhase;

const SEARCH_CHUNK_SIZE: usize = 128 * 1024;

/// What the renderer sees at a given original file offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteSlot {
    Present(u8),
    Deleted,
    Empty,
}

/// Backing document model for the editor.
///
/// The file is read lazily through `FileView`, while edits are recorded in
/// `PatchSet`. Deletions remain logical tombstones until a rewrite save.
#[derive(Debug)]
pub struct Document {
    path: PathBuf,
    readonly: bool,
    page_size: usize,
    cache_pages: usize,
    original_len: u64,
    view: FileView,
    patches: PatchSet,
}

impl Document {
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
            patches: PatchSet::default(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn original_len(&self) -> u64 {
        self.original_len
    }

    pub fn len(&self) -> u64 {
        self.patches
            .max_touched_offset()
            .map(|offset| offset + 1)
            .unwrap_or(self.original_len)
            .max(self.original_len)
    }

    pub fn visible_len(&self) -> u64 {
        self.len()
            .saturating_sub(self.patches.deletions().len() as u64)
    }

    pub fn is_dirty(&self) -> bool {
        self.patches.is_dirty()
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    pub fn has_deletions(&self) -> bool {
        self.patches.has_deletions()
    }

    pub fn has_appends(&self) -> bool {
        self.patches.has_appends(self.original_len)
    }

    pub fn patches(&self) -> &PatchSet {
        &self.patches
    }

    pub fn io_stats(&self) -> CacheStats {
        self.view.cache_stats()
    }

    pub fn patch_state_at(&self, offset: u64) -> PatchState {
        self.patches.state_at(offset)
    }

    pub fn raw_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if offset >= self.original_len {
            return Ok(Vec::new());
        }
        let clamped = len.min((self.original_len - offset) as usize);
        self.view.read_range(offset, clamped)
    }

    pub fn byte_at(&mut self, offset: u64) -> HxResult<ByteSlot> {
        // Patches always shadow on-disk bytes for both rendering and search.
        if self.patches.is_deleted(offset) {
            return Ok(ByteSlot::Deleted);
        }
        if let Some(value) = self.patches.replacement_at(offset) {
            return Ok(ByteSlot::Present(value));
        }
        if offset >= self.original_len {
            return Ok(ByteSlot::Empty);
        }
        let raw = self.view.read_range(offset, 1)?;
        Ok(raw
            .first()
            .copied()
            .map(ByteSlot::Present)
            .unwrap_or(ByteSlot::Empty))
    }

    pub fn byte_for_edit(&mut self, offset: u64) -> HxResult<u8> {
        if let Some(value) = self.patches.replacement_at(offset) {
            return Ok(value);
        }
        if offset >= self.original_len {
            return Err(HxError::OffsetOutOfRange);
        }
        let raw = self.view.read_range(offset, 1)?;
        raw.first().copied().ok_or(HxError::OffsetOutOfRange)
    }

    pub fn row_bytes(&mut self, offset: u64, width: usize) -> HxResult<Vec<ByteSlot>> {
        let mut out = Vec::with_capacity(width);
        for idx in 0..width {
            out.push(self.byte_at(offset + idx as u64)?);
        }
        Ok(out)
    }

    pub fn replace_nibble(&mut self, offset: u64, phase: NibblePhase, nibble: u8) -> HxResult<u64> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset == self.len() {
            if matches!(phase, NibblePhase::High) {
                self.set_byte(offset, nibble << 4)?;
                return Ok(offset);
            }
            return Err(HxError::OffsetOutOfRange);
        }
        if offset >= self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        let current = self.byte_for_edit(offset)?;
        let updated = match phase {
            NibblePhase::High => (nibble << 4) | (current & 0x0f),
            NibblePhase::Low => (current & 0xf0) | nibble,
        };
        if offset >= self.original_len {
            self.patches.set_replacement(offset, updated);
            return Ok(offset);
        }
        let original = self.raw_byte(offset)?;
        if updated == original {
            self.patches.apply_state(offset, PatchState::Unmodified);
        } else {
            self.patches.set_replacement(offset, updated);
        }
        Ok(offset)
    }

    pub fn delete_byte(&mut self, offset: u64) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset >= self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        self.patches.mark_deleted(offset);
        Ok(())
    }

    pub fn restore_patch_state(&mut self, offset: u64, state: PatchState) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        self.patches.apply_state(offset, state);
        Ok(())
    }

    pub fn set_byte(&mut self, offset: u64, value: u8) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset > self.len() {
            return Err(HxError::OffsetOutOfRange);
        }

        if offset < self.original_len {
            let original = self.raw_byte(offset)?;
            if value == original {
                self.patches.apply_state(offset, PatchState::Unmodified);
            } else {
                self.patches.set_replacement(offset, value);
            }
        } else {
            self.patches.set_replacement(offset, value);
        }

        Ok(())
    }

    pub fn save(&mut self, path: Option<PathBuf>) -> HxResult<PathBuf> {
        let target = path.unwrap_or_else(|| self.path.clone());
        if target == self.path && self.readonly {
            return Err(HxError::ReadOnly);
        }

        // Fixed-size sessions can patch the file in place. Any logical deletion
        // requires rewriting the compacted byte stream.
        if target == self.path && !self.has_deletions() && !self.has_appends() {
            save::save_in_place(self, &target)?;
        } else {
            save::save_rewrite(self, &target)?;
        }

        self.path = target.clone();
        self.view
            .reload(&target, self.readonly, self.page_size, self.cache_pages)?;
        self.original_len = self.view.len();
        self.patches.clear();
        Ok(target)
    }

    pub fn goto(&self, offset: u64) -> HxResult<u64> {
        if self.len() == 0 {
            return Ok(0);
        }
        if offset >= self.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok(offset)
    }

    pub fn search_forward(&mut self, start: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        if pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        if start >= self.len() {
            return Ok(None);
        }

        let overlap = pattern.len().saturating_sub(1);
        let mut carry_offsets = Vec::with_capacity(overlap);
        let mut carry_bytes = Vec::with_capacity(overlap);
        let mut offset = start;
        let limit = self.len();

        while offset < limit {
            let len = SEARCH_CHUNK_SIZE.min((limit - offset) as usize);
            let (logical_offsets, logical_bytes) = self.logical_chunk(offset, len)?;
            if logical_bytes.is_empty() {
                break;
            }

            let mut chunk_offsets = std::mem::take(&mut carry_offsets);
            let mut chunk_bytes = std::mem::take(&mut carry_bytes);
            chunk_offsets.extend(logical_offsets);
            chunk_bytes.extend(logical_bytes);

            if let Some(idx) = search::find(&chunk_bytes, pattern) {
                return Ok(Some(chunk_offsets[idx]));
            }

            let keep = overlap.min(chunk_bytes.len());
            if keep > 0 {
                carry_offsets = chunk_offsets[chunk_offsets.len() - keep..].to_vec();
                carry_bytes = chunk_bytes[chunk_bytes.len() - keep..].to_vec();
            }

            offset += len as u64;
        }
        Ok(None)
    }

    pub fn search_backward(&mut self, end_exclusive: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        if pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        let mut end = end_exclusive.min(self.len());
        if end == 0 {
            return Ok(None);
        }

        let overlap = pattern.len().saturating_sub(1);
        let mut carry_offsets = Vec::with_capacity(overlap);
        let mut carry_bytes = Vec::with_capacity(overlap);

        while end > 0 {
            let start = end.saturating_sub(SEARCH_CHUNK_SIZE as u64);
            let len = (end - start) as usize;
            let (mut logical_offsets, mut logical_bytes) = self.logical_chunk(start, len)?;
            if logical_bytes.is_empty() {
                break;
            }

            logical_offsets.extend_from_slice(&carry_offsets);
            logical_bytes.extend_from_slice(&carry_bytes);

            if let Some(idx) = search::rfind(&logical_bytes, pattern) {
                let found = logical_offsets[idx];
                if found < end_exclusive {
                    return Ok(Some(found));
                }
            }

            let keep = overlap.min(logical_bytes.len());
            if keep > 0 {
                carry_offsets = logical_offsets[..keep].to_vec();
                carry_bytes = logical_bytes[..keep].to_vec();
            }

            end = start;
        }

        Ok(None)
    }

    pub fn logical_bytes(&mut self, start: u64, end_inclusive: u64) -> HxResult<Vec<u8>> {
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(Vec::new());
        }

        let end = end_inclusive.min(len - 1);
        let (_, bytes) = self.logical_chunk(start, (end - start + 1) as usize)?;
        Ok(bytes)
    }

    fn raw_byte(&mut self, offset: u64) -> HxResult<u8> {
        let raw = self.view.read_range(offset, 1)?;
        raw.first().copied().ok_or(HxError::OffsetOutOfRange)
    }

    fn materialize_logical_chunk(
        &self,
        chunk_start: u64,
        raw: Vec<u8>,
        offsets: &mut Vec<u64>,
        bytes: &mut Vec<u8>,
    ) {
        offsets.reserve(raw.len());
        bytes.reserve(raw.len());
        for (idx, raw_byte) in raw.into_iter().enumerate() {
            let absolute = chunk_start + idx as u64;
            if self.patches.is_deleted(absolute) {
                continue;
            }
            offsets.push(absolute);
            bytes.push(self.patches.replacement_at(absolute).unwrap_or(raw_byte));
        }
    }

    fn logical_chunk(&mut self, start: u64, len: usize) -> HxResult<(Vec<u64>, Vec<u8>)> {
        let limit = self.len();
        if start >= limit || len == 0 {
            return Ok((Vec::new(), Vec::new()));
        }

        let end = (start + len as u64).min(limit);
        let mut offsets = Vec::with_capacity(len);
        let mut bytes = Vec::with_capacity(len);

        if start < self.original_len {
            let raw_end = end.min(self.original_len);
            let raw = self.raw_range(start, (raw_end - start) as usize)?;
            self.materialize_logical_chunk(start, raw, &mut offsets, &mut bytes);
        }

        if end > self.original_len {
            let append_start = start.max(self.original_len);
            for offset in append_start..end {
                if self.patches.is_deleted(offset) {
                    continue;
                }
                if let Some(value) = self.patches.replacement_at(offset) {
                    offsets.push(offset);
                    bytes.push(value);
                }
            }
        }

        Ok((offsets, bytes))
    }
}
