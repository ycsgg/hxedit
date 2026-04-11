use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

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

/// Small page cache to avoid repeated seek/read calls while scrolling.
#[derive(Debug)]
pub struct PageCache {
    page_size: usize,
    capacity: usize,
    entries: HashMap<u64, Vec<u8>>,
    order: VecDeque<u64>,
    stats: CacheStats,
}

impl PageCache {
    pub fn new(page_size: usize, capacity: usize) -> Self {
        Self {
            page_size,
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
            stats: CacheStats::default(),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    pub fn read_range(&mut self, file: &mut File, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        self.stats.read_range_calls += 1;

        let start_page = offset / self.page_size as u64;
        let end_page = (offset + len.saturating_sub(1) as u64) / self.page_size as u64;
        let mut out = Vec::with_capacity(len);

        for page_idx in start_page..=end_page {
            let page = self.load_page(file, page_idx)?;
            let page_start = page_idx * self.page_size as u64;
            let slice_start = if offset > page_start {
                (offset - page_start) as usize
            } else {
                0
            };
            let wanted_end = (offset + len as u64).min(page_start + page.len() as u64);
            let slice_end = wanted_end.saturating_sub(page_start) as usize;
            if slice_start < slice_end && slice_end <= page.len() {
                out.extend_from_slice(&page[slice_start..slice_end]);
            }
        }

        self.stats.bytes_returned += out.len() as u64;
        Ok(out)
    }

    fn load_page(&mut self, file: &mut File, page_idx: u64) -> io::Result<Vec<u8>> {
        if let Some(buf) = self.entries.get(&page_idx).cloned() {
            self.stats.page_hits += 1;
            self.touch(page_idx);
            return Ok(buf);
        }

        // Read a whole page at once so adjacent cursor moves are cheap.
        self.stats.page_misses += 1;
        let page_start = page_idx * self.page_size as u64;
        file.seek(SeekFrom::Start(page_start))?;
        let mut buf = vec![0; self.page_size];
        let read = file.read(&mut buf)?;
        buf.truncate(read);

        self.entries.insert(page_idx, buf.clone());
        self.touch(page_idx);
        self.evict_if_needed();
        Ok(buf)
    }

    fn touch(&mut self, page_idx: u64) {
        if let Some(pos) = self.order.iter().position(|idx| *idx == page_idx) {
            self.order.remove(pos);
        }
        self.order.push_back(page_idx);
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}
