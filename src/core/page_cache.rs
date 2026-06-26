use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::error::HxResult;

#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    pub read_range_calls: u64,
    pub page_hits: u64,
    pub page_misses: u64,
    pub bytes_returned: u64,
}

impl CacheStats {
    pub fn delta_from(self, previous: Self) -> Self {
        Self {
            read_range_calls: self
                .read_range_calls
                .saturating_sub(previous.read_range_calls),
            page_hits: self.page_hits.saturating_sub(previous.page_hits),
            page_misses: self.page_misses.saturating_sub(previous.page_misses),
            bytes_returned: self.bytes_returned.saturating_sub(previous.bytes_returned),
        }
    }
}

/// A cached page along with its last-touched generation counter.
#[derive(Debug)]
struct CachedPage {
    data: Vec<u8>,
    last_used: u64,
}

/// Small page cache to avoid repeated seek/read calls while scrolling.
///
/// LRU is approximated via a monotonic generation counter: every hit or miss
/// bumps `generation` and stores it on the touched page. Eviction scans the
/// (capacity-bounded) entries to drop the oldest. This keeps touch O(1) — the
/// hot path during sequential scrolling — and pays an O(capacity) tax only
/// when we evict, which happens at most once per miss.
#[derive(Debug)]
pub struct PageCache {
    page_size: usize,
    capacity: usize,
    entries: HashMap<u64, CachedPage>,
    generation: u64,
    stats: CacheStats,
}

impl PageCache {
    pub fn new(page_size: usize, capacity: usize) -> Self {
        Self {
            page_size: page_size.max(1),
            capacity: capacity.max(1),
            entries: HashMap::new(),
            generation: 0,
            stats: CacheStats::default(),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.generation = 0;
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    pub fn read_range(&mut self, file: &mut File, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        self.read_range_with(offset, len, |page_start, page_size| {
            file.seek(SeekFrom::Start(page_start))?;
            let mut buf = vec![0; page_size];
            let read = file.read(&mut buf)?;
            buf.truncate(read);
            Ok(buf)
        })
    }

    pub fn read_range_with(
        &mut self,
        offset: u64,
        len: usize,
        mut load_page: impl FnMut(u64, usize) -> HxResult<Vec<u8>>,
    ) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        self.stats.read_range_calls += 1;

        let start_page = offset / self.page_size as u64;
        let end_page = offset.saturating_add(len.saturating_sub(1) as u64) / self.page_size as u64;
        let requested_end = offset.saturating_add(len as u64);

        let mut out = Vec::with_capacity(len);
        for page_idx in start_page..=end_page {
            self.ensure_loaded_with(page_idx, &mut load_page)?;
            let Some(entry) = self.entries.get(&page_idx) else {
                continue;
            };
            let page = &entry.data;
            let page_start = page_idx * self.page_size as u64;
            let slice_start = if offset > page_start {
                (offset - page_start) as usize
            } else {
                0
            };
            let wanted_end = requested_end.min(page_start.saturating_add(page.len() as u64));
            let slice_end = wanted_end.saturating_sub(page_start) as usize;
            if slice_start < slice_end && slice_end <= page.len() {
                out.extend_from_slice(&page[slice_start..slice_end]);
            }
        }

        self.stats.bytes_returned += out.len() as u64;
        Ok(out)
    }

    fn ensure_loaded_with(
        &mut self,
        page_idx: u64,
        load_page: &mut impl FnMut(u64, usize) -> HxResult<Vec<u8>>,
    ) -> HxResult<()> {
        if let Some(entry) = self.entries.get_mut(&page_idx) {
            self.stats.page_hits += 1;
            self.generation += 1;
            entry.last_used = self.generation;
            return Ok(());
        }

        self.stats.page_misses += 1;
        let page_start = page_idx * self.page_size as u64;
        let buf = load_page(page_start, self.page_size)?;

        self.generation += 1;
        self.entries.insert(
            page_idx,
            CachedPage {
                data: buf,
                last_used: self.generation,
            },
        );
        self.evict_if_needed();
        Ok(())
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.capacity {
            if let Some((&victim, _)) = self.entries.iter().min_by_key(|(_, entry)| entry.last_used)
            {
                self.entries.remove(&victim);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::PageCache;

    #[test]
    fn read_range_can_span_more_pages_than_cache_capacity() {
        let mut temp = NamedTempFile::new().unwrap();
        let data = (0..64).collect::<Vec<u8>>();
        temp.write_all(&data).unwrap();

        let mut file = temp.reopen().unwrap();
        let mut cache = PageCache::new(4, 2);
        let read = cache.read_range(&mut file, 0, data.len()).unwrap();

        assert_eq!(read, data);
    }

    #[test]
    fn zero_sized_cache_configuration_is_clamped() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"abc").unwrap();

        let mut file = temp.reopen().unwrap();
        let mut cache = PageCache::new(0, 0);
        let read = cache.read_range(&mut file, 0, 3).unwrap();

        assert_eq!(read, b"abc");
    }
}
