use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::core::page_cache::{CacheStats, PageCache};
use crate::error::{HxError, HxResult};

/// Read-through file access with page caching.
#[derive(Debug)]
pub struct FileView {
    path: PathBuf,
    file: File,
    len: u64,
    cache: PageCache,
}

impl FileView {
    pub fn open(
        path: &Path,
        readonly: bool,
        page_size: usize,
        cache_pages: usize,
    ) -> HxResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(!readonly)
            .open(path)
            .map_err(|source| HxError::OpenPath {
                path: path.to_path_buf(),
                source,
            })?;
        let len = file.metadata()?.len();
        Ok(Self {
            path: path.to_path_buf(),
            file,
            len,
            cache: PageCache::new(page_size, cache_pages),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn read_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        Ok(self.cache.read_range(&mut self.file, offset, len)?)
    }

    pub fn reload(
        &mut self,
        path: &Path,
        readonly: bool,
        page_size: usize,
        cache_pages: usize,
    ) -> HxResult<()> {
        *self = Self::open(path, readonly, page_size, cache_pages)?;
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }
}
