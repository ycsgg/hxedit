use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::core::page_cache::{CacheStats, PageCache};
use crate::error::{HxError, HxResult};
use crate::remote::{RemoteSave, RemoteSource, RemoteStat, RemoteTarget};

const REMOTE_MIN_PAGE_SIZE: usize = 1024 * 1024;

/// Read-through file access with page caching.
#[derive(Debug)]
pub struct FileView {
    path: PathBuf,
    storage: FileStorage,
    len: u64,
    cache: PageCache,
}

#[derive(Debug)]
enum FileStorage {
    Disk(File),
    Memory(Vec<u8>),
    Remote(Box<RemoteSource>),
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
            storage: FileStorage::Disk(file),
            len,
            cache: PageCache::new(page_size, cache_pages),
        })
    }

    pub fn from_bytes(path: PathBuf, bytes: Vec<u8>, page_size: usize, cache_pages: usize) -> Self {
        Self {
            path,
            len: bytes.len() as u64,
            storage: FileStorage::Memory(bytes),
            cache: PageCache::new(page_size, cache_pages),
        }
    }

    pub fn open_remote(
        target: RemoteTarget,
        readonly: bool,
        page_size: usize,
        cache_pages: usize,
    ) -> HxResult<Self> {
        let source = RemoteSource::open(target, readonly)?;
        let label = source.label();
        let len = source.len();
        let page_size = page_size.max(REMOTE_MIN_PAGE_SIZE);
        Ok(Self {
            path: PathBuf::from(label),
            len,
            storage: FileStorage::Remote(Box::new(source)),
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
        match &mut self.storage {
            FileStorage::Disk(file) => self.cache.read_range(file, offset, len),
            FileStorage::Memory(bytes) => {
                let start = offset as usize;
                if start >= bytes.len() {
                    return Ok(Vec::new());
                }
                let end = start.saturating_add(len).min(bytes.len());
                Ok(bytes[start..end].to_vec())
            }
            FileStorage::Remote(source) => {
                self.cache
                    .read_range_with(offset, len, |page_start, page_size| {
                        source.read_at(page_start, page_size)
                    })
            }
        }
    }

    pub(crate) fn read_remote_direct(
        &mut self,
        offset: u64,
        len: usize,
    ) -> HxResult<Option<Vec<u8>>> {
        let FileStorage::Remote(source) = &mut self.storage else {
            return Ok(None);
        };
        if len == 0 || offset >= self.len {
            return Ok(Some(Vec::new()));
        }
        let clamped = len.min((self.len - offset) as usize);
        source.read_at(offset, clamped).map(Some)
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

    pub fn reload_current(
        &mut self,
        readonly: bool,
        page_size: usize,
        cache_pages: usize,
    ) -> HxResult<()> {
        if matches!(self.storage, FileStorage::Disk(_)) {
            let path = self.path.clone();
            return self.reload(&path, readonly, page_size, cache_pages);
        }

        match &mut self.storage {
            FileStorage::Disk(_) => unreachable!("disk storage returned above"),
            FileStorage::Memory(bytes) => {
                self.len = bytes.len() as u64;
                self.cache = PageCache::new(page_size, cache_pages);
                Ok(())
            }
            FileStorage::Remote(source) => {
                let stat = source.reload()?;
                self.len = stat.len;
                self.path = PathBuf::from(source.label());
                self.cache = PageCache::new(page_size.max(REMOTE_MIN_PAGE_SIZE), cache_pages);
                Ok(())
            }
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self.storage, FileStorage::Remote(_))
    }

    pub fn is_remote_readonly(&self) -> Option<bool> {
        match &self.storage {
            FileStorage::Remote(source) => Some(source.readonly()),
            FileStorage::Disk(_) | FileStorage::Memory(_) => None,
        }
    }

    pub fn label(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    pub fn begin_remote_save(&self) -> HxResult<Box<dyn RemoteSave>> {
        match &self.storage {
            FileStorage::Remote(source) => source.begin_save(),
            FileStorage::Disk(_) | FileStorage::Memory(_) => {
                Err(HxError::Remote("current document is not remote".to_owned()))
            }
        }
    }

    pub fn complete_remote_save(&mut self, stat: RemoteStat) -> HxResult<()> {
        match &mut self.storage {
            FileStorage::Remote(source) => {
                source.complete_save(stat);
                let stat = source.reload()?;
                self.len = stat.len;
                self.path = PathBuf::from(source.label());
                self.cache.clear();
                Ok(())
            }
            FileStorage::Disk(_) | FileStorage::Memory(_) => {
                Err(HxError::Remote("current document is not remote".to_owned()))
            }
        }
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }
}
