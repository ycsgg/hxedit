use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::core::file_view::FileView;
use crate::core::patch::PatchSet;
use crate::core::save;
use crate::core::search::KmpSearcher;
use crate::error::{HxError, HxResult};
use crate::mode::NibblePhase;

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

    pub fn visible_len(&self) -> u64 {
        self.original_len
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

    pub fn patches(&self) -> &PatchSet {
        &self.patches
    }

    pub fn raw_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if offset >= self.original_len {
            return Ok(Vec::new());
        }
        let clamped = len.min((self.original_len - offset) as usize);
        self.view.read_range(offset, clamped)
    }

    pub fn byte_at(&mut self, offset: u64) -> HxResult<ByteSlot> {
        if offset >= self.original_len {
            return Ok(ByteSlot::Empty);
        }
        // Patches always shadow on-disk bytes for both rendering and search.
        if self.patches.is_deleted(offset) {
            return Ok(ByteSlot::Deleted);
        }
        if let Some(value) = self.patches.replacement_at(offset) {
            return Ok(ByteSlot::Present(value));
        }
        let raw = self.view.read_range(offset, 1)?;
        Ok(raw
            .first()
            .copied()
            .map(ByteSlot::Present)
            .unwrap_or(ByteSlot::Empty))
    }

    pub fn byte_for_edit(&mut self, offset: u64) -> HxResult<u8> {
        if offset >= self.original_len {
            return Err(HxError::OffsetOutOfRange);
        }
        if let Some(value) = self.patches.replacement_at(offset) {
            return Ok(value);
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
        if offset >= self.original_len {
            return Err(HxError::OffsetOutOfRange);
        }
        let current = self.byte_for_edit(offset)?;
        let updated = match phase {
            NibblePhase::High => (nibble << 4) | (current & 0x0f),
            NibblePhase::Low => (current & 0xf0) | nibble,
        };
        self.patches.set_replacement(offset, updated);
        Ok(offset)
    }

    pub fn delete_byte(&mut self, offset: u64) -> HxResult<()> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        if offset >= self.original_len {
            return Err(HxError::OffsetOutOfRange);
        }
        self.patches.mark_deleted(offset);
        Ok(())
    }

    pub fn save(&mut self, path: Option<PathBuf>) -> HxResult<PathBuf> {
        let target = path.unwrap_or_else(|| self.path.clone());
        if target == self.path && self.readonly {
            return Err(HxError::ReadOnly);
        }

        // Fixed-size sessions can patch the file in place. Any logical deletion
        // requires rewriting the compacted byte stream.
        if target == self.path && !self.has_deletions() {
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
        if self.original_len == 0 {
            return Ok(0);
        }
        if offset >= self.original_len {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok(offset)
    }

    pub fn search_forward(&mut self, start: u64, pattern: &[u8]) -> HxResult<Option<u64>> {
        if pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        if start >= self.original_len {
            return Ok(None);
        }

        let mut searcher = KmpSearcher::new(pattern.to_vec());
        let mut offset = start;
        let chunk_size = 64 * 1024;

        while offset < self.original_len {
            let bytes = self.raw_range(offset, chunk_size)?;
            if bytes.is_empty() {
                break;
            }
            for (idx, byte) in bytes.into_iter().enumerate() {
                let absolute = offset + idx as u64;
                // Deleted bytes are removed from the logical stream, so they
                // break any in-flight match.
                if self.patches.is_deleted(absolute) {
                    searcher.reset();
                    continue;
                }
                let value = self.patches.replacement_at(absolute).unwrap_or(byte);
                if searcher.feed(value) {
                    let found = absolute + 1 - searcher.len() as u64;
                    return Ok(Some(found));
                }
            }
            offset += chunk_size as u64;
        }
        Ok(None)
    }
}
