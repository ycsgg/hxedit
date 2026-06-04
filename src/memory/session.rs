use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::config::Config;
use crate::core::document::Document;
use crate::error::{HxError, HxResult};

use super::{
    MemoryBackend, MemoryRegion, MemorySearchDirection, MemorySearchHit, MemorySearchQuery,
    ProcessFingerprint, ProcessInfo, RegionFingerprint,
};

pub const PAGE_SIZE: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemorySessionState {
    Alive,
    Frozen { depth: usize },
    Dead(String),
}

#[derive(Debug, Clone)]
struct RegionState {
    region: MemoryRegion,
    dirty_bytes: usize,
    stale_base: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PageKey {
    fingerprint: RegionFingerprint,
    page_va: u64,
}

#[derive(Debug, Clone)]
struct PageSnapshot {
    start_va: u64,
    bytes: Vec<u8>,
}

pub struct MemorySession {
    backend: Box<dyn MemoryBackend>,
    process_info: ProcessInfo,
    process_fingerprint: ProcessFingerprint,
    regions: Vec<RegionState>,
    pages: BTreeMap<PageKey, PageSnapshot>,
    state: MemorySessionState,
}

impl MemorySession {
    pub fn open(mut backend: Box<dyn MemoryBackend>) -> HxResult<Self> {
        let process_info = backend.process_info()?;
        let process_fingerprint = backend.process_fingerprint()?;
        let regions = backend
            .memory_regions()?
            .into_iter()
            .map(|region| RegionState {
                region,
                dirty_bytes: 0,
                stale_base: false,
            })
            .collect();

        Ok(Self {
            backend,
            process_info,
            process_fingerprint,
            regions,
            pages: BTreeMap::new(),
            state: MemorySessionState::Alive,
        })
    }

    pub fn process_info(&self) -> &ProcessInfo {
        &self.process_info
    }

    pub fn state(&self) -> &MemorySessionState {
        &self.state
    }

    pub fn is_dead(&self) -> bool {
        matches!(self.state, MemorySessionState::Dead(_))
    }

    pub fn is_frozen(&self) -> bool {
        matches!(self.state, MemorySessionState::Frozen { .. })
    }

    pub fn freeze_depth(&self) -> usize {
        match self.state {
            MemorySessionState::Frozen { depth } => depth,
            MemorySessionState::Alive | MemorySessionState::Dead(_) => 0,
        }
    }

    pub fn regions(&self) -> impl Iterator<Item = &MemoryRegion> {
        self.regions.iter().map(|state| &state.region)
    }

    pub fn region(&self, index: usize) -> Option<&MemoryRegion> {
        self.regions.get(index).map(|state| &state.region)
    }

    pub fn region_dirty_bytes(&self, index: usize) -> Option<usize> {
        self.regions.get(index).map(|state| state.dirty_bytes)
    }

    pub fn region_stale_base(&self, index: usize) -> Option<bool> {
        self.regions.get(index).map(|state| state.stale_base)
    }

    pub fn cached_page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn mark_region_dirty(&mut self, index: usize, dirty_bytes: usize) -> HxResult<()> {
        let Some(region) = self.regions.get_mut(index) else {
            return Err(HxError::OffsetOutOfRange);
        };
        region.dirty_bytes = dirty_bytes;
        Ok(())
    }

    pub fn clear_region_dirty(&mut self, index: usize) -> HxResult<()> {
        let Some(region) = self.regions.get_mut(index) else {
            return Err(HxError::OffsetOutOfRange);
        };
        region.dirty_bytes = 0;
        region.stale_base = false;
        Ok(())
    }

    pub fn freeze(&mut self) -> HxResult<()> {
        self.ensure_alive()?;
        match &mut self.state {
            MemorySessionState::Frozen { depth } => {
                *depth = depth.saturating_add(1);
                Ok(())
            }
            MemorySessionState::Alive => {
                self.ensure_freeze_allowed()?;
                self.backend.freeze()?;
                self.state = MemorySessionState::Frozen { depth: 1 };
                Ok(())
            }
            MemorySessionState::Dead(_) => unreachable!("ensure_alive returned for dead session"),
        }
    }

    pub fn thaw(&mut self) -> HxResult<()> {
        match self.state.clone() {
            MemorySessionState::Dead(reason) => Err(HxError::ProcessDead(reason)),
            MemorySessionState::Alive => Ok(()),
            MemorySessionState::Frozen { depth } if depth > 1 => {
                self.state = MemorySessionState::Frozen { depth: depth - 1 };
                Ok(())
            }
            MemorySessionState::Frozen { .. } => {
                self.backend.thaw()?;
                self.state = MemorySessionState::Alive;
                Ok(())
            }
        }
    }

    pub fn read_region_range(
        &mut self,
        region_index: usize,
        offset: u64,
        len: usize,
    ) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        self.ensure_alive()?;

        let region = self
            .regions
            .get(region_index)
            .map(|state| state.region.clone())
            .ok_or(HxError::OffsetOutOfRange)?;
        if !region.permissions.read {
            return Err(HxError::MemoryAccess {
                addr: region.start.saturating_add(offset),
                len,
                message: "region is not readable".to_owned(),
            });
        }
        let addr = region
            .start
            .checked_add(offset)
            .ok_or(HxError::OffsetOutOfRange)?;
        if !region.contains_range(addr, len) {
            return Err(HxError::OffsetOutOfRange);
        }

        let mut out = Vec::with_capacity(len);
        let mut cursor = addr;
        let end = addr + len as u64;
        while cursor < end {
            let page = self.page_for_addr(&region, cursor)?;
            let page_end = page.start_va + page.bytes.len() as u64;
            let take_end = page_end.min(end);
            let start_index = (cursor - page.start_va) as usize;
            let end_index = (take_end - page.start_va) as usize;
            out.extend_from_slice(&page.bytes[start_index..end_index]);
            cursor = take_end;
        }
        Ok(out)
    }

    fn read_region_range_uncached(
        &mut self,
        region_index: usize,
        addr: u64,
        len: usize,
    ) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let region = self
            .regions
            .get(region_index)
            .map(|state| state.region.clone())
            .ok_or(HxError::OffsetOutOfRange)?;
        if !region.permissions.read {
            return Err(HxError::MemoryAccess {
                addr,
                len,
                message: "region is not readable".to_owned(),
            });
        }
        if !region.contains_range(addr, len) {
            return Err(HxError::OffsetOutOfRange);
        }
        let mut bytes = vec![0; len];
        self.backend.read_at(addr, &mut bytes)?;
        Ok(bytes)
    }

    pub fn document_for_region(
        &mut self,
        region_index: usize,
        config: &Config,
    ) -> HxResult<Document> {
        let region = self
            .regions
            .get(region_index)
            .map(|state| state.region.clone())
            .ok_or(HxError::OffsetOutOfRange)?;
        if !region.permissions.read {
            return Err(HxError::MemoryAccess {
                addr: region.start,
                len: region.len() as usize,
                message: "region is not readable".to_owned(),
            });
        }
        let bytes = self.read_region_range(region_index, 0, region.len() as usize)?;
        Ok(Document::from_memory_bytes(
            memory_document_path(self.process_info.pid, &region),
            bytes,
            config,
        ))
    }

    pub fn write_at(&mut self, addr: u64, data: &[u8]) -> HxResult<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.ensure_alive()?;
        let Some(index) = self.region_index_containing_range(addr, data.len()) else {
            return Err(HxError::OffsetOutOfRange);
        };
        if !self.regions[index].region.permissions.write {
            return Err(HxError::MemoryAccess {
                addr,
                len: data.len(),
                message: "region is not writable".to_owned(),
            });
        }
        self.backend.write_at(addr, data)?;
        self.patch_cached_pages(addr, data);
        self.regions[index].dirty_bytes += data.len();
        Ok(())
    }

    pub fn search(
        &mut self,
        query: &MemorySearchQuery,
        start_addr: Option<u64>,
        direction: MemorySearchDirection,
    ) -> HxResult<Option<MemorySearchHit>> {
        if query.pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        self.ensure_alive()?;
        let Some(start_index) = start_addr
            .and_then(|addr| {
                self.regions
                    .iter()
                    .position(|state| state.region.contains(addr))
            })
            .or_else(|| {
                self.regions
                    .iter()
                    .position(|state| query.filter.matches(&state.region))
            })
        else {
            return Ok(None);
        };
        match direction {
            MemorySearchDirection::Forward => self.search_forward(query, start_index, start_addr),
            MemorySearchDirection::Backward => self.search_backward(query, start_index, start_addr),
        }
    }

    pub fn refresh_regions(&mut self) -> HxResult<()> {
        self.ensure_alive()?;
        let refreshed = self.backend.memory_regions()?;
        let mut next = Vec::with_capacity(refreshed.len());
        let mut keep_fingerprints = BTreeSet::new();

        for region in refreshed {
            let previous = self
                .regions
                .iter()
                .find(|state| same_region_identity(&state.region, &region));
            let state = match previous {
                Some(previous) if previous.region.fingerprint == region.fingerprint => {
                    keep_fingerprints.insert(region.fingerprint);
                    RegionState {
                        region,
                        dirty_bytes: previous.dirty_bytes,
                        stale_base: previous.stale_base,
                    }
                }
                Some(previous) if previous.dirty_bytes > 0 => {
                    keep_fingerprints.insert(previous.region.fingerprint);
                    RegionState {
                        region: previous.region.clone(),
                        dirty_bytes: previous.dirty_bytes,
                        stale_base: true,
                    }
                }
                _ => RegionState {
                    region,
                    dirty_bytes: 0,
                    stale_base: false,
                },
            };
            next.push(state);
        }

        for previous in &self.regions {
            let removed = !next
                .iter()
                .any(|state| same_region_identity(&state.region, &previous.region));
            if removed && previous.dirty_bytes > 0 {
                keep_fingerprints.insert(previous.region.fingerprint);
                let mut region = previous.clone();
                region.stale_base = true;
                next.push(region);
            }
        }

        self.pages
            .retain(|key, _| keep_fingerprints.contains(&key.fingerprint));
        self.regions = next;
        Ok(())
    }

    fn ensure_alive(&mut self) -> HxResult<()> {
        if let MemorySessionState::Dead(reason) = &self.state {
            return Err(HxError::ProcessDead(reason.clone()));
        }
        let current = self.backend.process_fingerprint()?;
        if current != self.process_fingerprint {
            let reason = "PID reuse or process exit detected".to_owned();
            self.state = MemorySessionState::Dead(reason.clone());
            return Err(HxError::ProcessDead(reason));
        }
        Ok(())
    }

    fn ensure_freeze_allowed(&self) -> HxResult<()> {
        let pid = self.process_info.pid;
        if pid == 1 {
            return Err(HxError::MemoryUnavailable(
                "refusing to freeze pid 1".to_owned(),
            ));
        }
        if pid == std::process::id() {
            return Err(HxError::MemoryUnavailable(
                "refusing to freeze the current hxedit process".to_owned(),
            ));
        }
        Ok(())
    }

    fn page_for_addr(&mut self, region: &MemoryRegion, addr: u64) -> HxResult<PageSnapshot> {
        let page_va = align_down(addr, PAGE_SIZE);
        let key = PageKey {
            fingerprint: region.fingerprint,
            page_va,
        };
        if let Some(page) = self.pages.get(&key) {
            return Ok(page.clone());
        }

        let start_va = page_va.max(region.start);
        let end_va = page_va.saturating_add(PAGE_SIZE).min(region.end);
        let mut bytes = vec![0; (end_va - start_va) as usize];
        self.backend.read_at(start_va, &mut bytes)?;
        let snapshot = PageSnapshot { start_va, bytes };
        self.pages.insert(key, snapshot.clone());
        Ok(snapshot)
    }

    fn region_index_containing_range(&self, addr: u64, len: usize) -> Option<usize> {
        self.regions
            .iter()
            .position(|state| state.region.contains_range(addr, len))
    }

    fn patch_cached_pages(&mut self, addr: u64, data: &[u8]) {
        let data_end = addr + data.len() as u64;
        for page in self.pages.values_mut() {
            let page_start = page.start_va;
            let page_end = page.start_va + page.bytes.len() as u64;
            let overlap_start = addr.max(page_start);
            let overlap_end = data_end.min(page_end);
            if overlap_start >= overlap_end {
                continue;
            }
            let data_start = (overlap_start - addr) as usize;
            let data_end = (overlap_end - addr) as usize;
            let page_start = (overlap_start - page.start_va) as usize;
            let page_end = (overlap_end - page.start_va) as usize;
            page.bytes[page_start..page_end].copy_from_slice(&data[data_start..data_end]);
        }
    }

    fn search_forward(
        &mut self,
        query: &MemorySearchQuery,
        start_index: usize,
        start_addr: Option<u64>,
    ) -> HxResult<Option<MemorySearchHit>> {
        let mut skipped = 0;
        let current = self.regions[start_index].region.clone();
        let start = start_addr
            .map(|addr| addr.saturating_add(1).min(current.end))
            .unwrap_or(current.start);
        if let Some(addr) =
            self.search_region_range(query, start_index, start, current.end, &mut skipped)?
        {
            return Ok(Some(MemorySearchHit {
                region_index: start_index,
                addr,
                wrapped: false,
                skipped_regions: skipped,
            }));
        }

        for index in start_index + 1..self.regions.len() {
            let region = self.regions[index].region.clone();
            if let Some(addr) =
                self.search_region_range(query, index, region.start, region.end, &mut skipped)?
            {
                return Ok(Some(MemorySearchHit {
                    region_index: index,
                    addr,
                    wrapped: false,
                    skipped_regions: skipped,
                }));
            }
        }

        for index in 0..start_index {
            let region = self.regions[index].region.clone();
            if let Some(addr) =
                self.search_region_range(query, index, region.start, region.end, &mut skipped)?
            {
                return Ok(Some(MemorySearchHit {
                    region_index: index,
                    addr,
                    wrapped: true,
                    skipped_regions: skipped,
                }));
            }
        }

        if let Some(start_addr) = start_addr {
            if current.start < start_addr {
                if let Some(addr) = self.search_region_range(
                    query,
                    start_index,
                    current.start,
                    start_addr,
                    &mut skipped,
                )? {
                    return Ok(Some(MemorySearchHit {
                        region_index: start_index,
                        addr,
                        wrapped: true,
                        skipped_regions: skipped,
                    }));
                }
            }
        }
        Ok(None)
    }

    fn search_backward(
        &mut self,
        query: &MemorySearchQuery,
        start_index: usize,
        start_addr: Option<u64>,
    ) -> HxResult<Option<MemorySearchHit>> {
        let mut skipped = 0;
        let current = self.regions[start_index].region.clone();
        let end = start_addr.unwrap_or(current.end).min(current.end);
        if let Some(addr) =
            self.search_region_range_backward(query, start_index, current.start, end, &mut skipped)?
        {
            return Ok(Some(MemorySearchHit {
                region_index: start_index,
                addr,
                wrapped: false,
                skipped_regions: skipped,
            }));
        }

        for index in (0..start_index).rev() {
            let region = self.regions[index].region.clone();
            if let Some(addr) = self.search_region_range_backward(
                query,
                index,
                region.start,
                region.end,
                &mut skipped,
            )? {
                return Ok(Some(MemorySearchHit {
                    region_index: index,
                    addr,
                    wrapped: false,
                    skipped_regions: skipped,
                }));
            }
        }

        for index in (start_index + 1..self.regions.len()).rev() {
            let region = self.regions[index].region.clone();
            if let Some(addr) = self.search_region_range_backward(
                query,
                index,
                region.start,
                region.end,
                &mut skipped,
            )? {
                return Ok(Some(MemorySearchHit {
                    region_index: index,
                    addr,
                    wrapped: true,
                    skipped_regions: skipped,
                }));
            }
        }

        if let Some(start_addr) = start_addr {
            if start_addr.saturating_add(1) < current.end {
                if let Some(addr) = self.search_region_range_backward(
                    query,
                    start_index,
                    start_addr.saturating_add(1),
                    current.end,
                    &mut skipped,
                )? {
                    return Ok(Some(MemorySearchHit {
                        region_index: start_index,
                        addr,
                        wrapped: true,
                        skipped_regions: skipped,
                    }));
                }
            }
        }
        Ok(None)
    }

    fn search_region_range(
        &mut self,
        query: &MemorySearchQuery,
        index: usize,
        start: u64,
        end: u64,
        skipped: &mut usize,
    ) -> HxResult<Option<u64>> {
        let region = self.regions[index].region.clone();
        if !query.filter.matches(&region) {
            *skipped += 1;
            return Ok(None);
        }
        let Some((start, end)) = query.filter.clamp_search_range(&region, start, end) else {
            return Ok(None);
        };
        let result = super::search::search_region_forward(
            |addr, len| self.read_region_range_uncached(index, addr, len),
            start,
            end,
            &query.pattern,
        );
        match result {
            Ok(hit) => Ok(hit),
            Err(HxError::MemoryAccess { .. }) => {
                *skipped += 1;
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    fn search_region_range_backward(
        &mut self,
        query: &MemorySearchQuery,
        index: usize,
        start: u64,
        end: u64,
        skipped: &mut usize,
    ) -> HxResult<Option<u64>> {
        let region = self.regions[index].region.clone();
        if !query.filter.matches(&region) {
            *skipped += 1;
            return Ok(None);
        }
        let Some((start, end)) = query.filter.clamp_search_range(&region, start, end) else {
            return Ok(None);
        };
        let result = super::search::search_region_backward(
            |addr, len| self.read_region_range_uncached(index, addr, len),
            start,
            end,
            &query.pattern,
        );
        match result {
            Ok(hit) => Ok(hit),
            Err(HxError::MemoryAccess { .. }) => {
                *skipped += 1;
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }
}

impl Drop for MemorySession {
    fn drop(&mut self) {
        if self.is_frozen() {
            let _ = self.backend.thaw();
            self.state = MemorySessionState::Alive;
        }
    }
}

fn same_region_identity(a: &MemoryRegion, b: &MemoryRegion) -> bool {
    a.start == b.start && a.end == b.end
}

fn align_down(value: u64, alignment: u64) -> u64 {
    value / alignment * alignment
}

fn memory_document_path(pid: u32, region: &MemoryRegion) -> PathBuf {
    PathBuf::from(format!(
        "memory://{pid}/0x{:x}-0x{:x}",
        region.start, region.end
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{FakeMemoryBackend, MemoryPermissions, RegionFingerprint, RegionKind};

    fn session_with_region(bytes: Vec<u8>) -> (MemorySession, FakeMemoryBackend) {
        let control = FakeMemoryBackend::new();
        control.add_region(
            0x1000,
            bytes,
            MemoryPermissions::read_write(),
            RegionFingerprint(1),
        );
        let session = MemorySession::open(Box::new(control.clone())).unwrap();
        (session, control)
    }

    #[test]
    fn read_region_range_uses_lazy_page_snapshots() {
        let (mut session, control) = session_with_region((0..32).collect());

        assert_eq!(
            session.read_region_range(0, 1, 4).unwrap(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(control.read_count(), 1);

        assert_eq!(session.read_region_range(0, 8, 2).unwrap(), vec![8, 9]);
        assert_eq!(control.read_count(), 1);
        assert_eq!(session.cached_page_count(), 1);
    }

    #[test]
    fn liveness_mismatch_marks_session_dead() {
        let (mut session, control) = session_with_region(vec![1, 2, 3, 4]);
        control.set_process_fingerprint(ProcessFingerprint(99));

        let err = session.read_region_range(0, 0, 1).unwrap_err();
        assert!(matches!(err, HxError::ProcessDead(_)));
        assert!(session.is_dead());
    }

    #[test]
    fn write_patches_cached_snapshot() {
        let (mut session, control) = session_with_region(vec![1, 2, 3, 4]);
        assert_eq!(
            session.read_region_range(0, 0, 4).unwrap(),
            vec![1, 2, 3, 4]
        );

        session.write_at(0x1001, &[9, 8]).unwrap();
        assert_eq!(
            session.read_region_range(0, 0, 4).unwrap(),
            vec![1, 9, 8, 4]
        );
        assert_eq!(control.write_count(), 1);
        assert_eq!(session.region_dirty_bytes(0), Some(2));
    }

    #[test]
    fn document_for_region_builds_fixed_size_document_from_snapshot() {
        let (mut session, control) = session_with_region(vec![0x12, 0x34, 0x56, 0x78]);
        let doc = session.document_for_region(0, &Config::default()).unwrap();

        assert!(doc.is_fixed_size());
        assert_eq!(doc.path().to_string_lossy(), "memory://4242/0x1000-0x1004");
        assert_eq!(doc.len(), 4);
        assert_eq!(doc.visible_len(), 4);
        assert_eq!(control.read_count(), 1);
    }

    #[test]
    fn refresh_drops_clean_changed_region_snapshots() {
        let (mut session, control) = session_with_region(vec![1, 2, 3, 4]);
        assert_eq!(session.read_region_range(0, 0, 1).unwrap(), vec![1]);
        assert_eq!(session.cached_page_count(), 1);

        let changed = MemoryRegion::new(
            0x1000,
            0x1004,
            MemoryPermissions::read_write(),
            RegionKind::Anonymous,
            RegionFingerprint(2),
        );
        control.set_regions(vec![changed]);

        session.refresh_regions().unwrap();
        assert_eq!(session.cached_page_count(), 0);
        assert_eq!(session.region_stale_base(0), Some(false));
    }

    #[test]
    fn refresh_marks_dirty_changed_region_stale() {
        let (mut session, control) = session_with_region(vec![1, 2, 3, 4]);
        session.mark_region_dirty(0, 1).unwrap();
        assert_eq!(session.read_region_range(0, 0, 1).unwrap(), vec![1]);

        let changed = MemoryRegion::new(
            0x1000,
            0x1004,
            MemoryPermissions::read_write(),
            RegionKind::Anonymous,
            RegionFingerprint(2),
        );
        control.set_regions(vec![changed]);

        session.refresh_regions().unwrap();
        assert_eq!(session.region_dirty_bytes(0), Some(1));
        assert_eq!(session.region_stale_base(0), Some(true));
        assert_eq!(session.cached_page_count(), 1);
    }

    #[test]
    fn memory_search_finds_hits_across_regions_and_wraps() {
        let control = FakeMemoryBackend::new();
        control.add_region(
            0x1000,
            b"abc".to_vec(),
            MemoryPermissions::readable(),
            RegionFingerprint(1),
        );
        control.add_region(
            0x2000,
            b"def".to_vec(),
            MemoryPermissions::readable(),
            RegionFingerprint(2),
        );
        let mut session = MemorySession::open(Box::new(control)).unwrap();
        let query = MemorySearchQuery::parse("/ab/").unwrap();

        let hit = session
            .search(&query, Some(0x2001), MemorySearchDirection::Forward)
            .unwrap()
            .unwrap();
        assert_eq!(hit.region_index, 0);
        assert_eq!(hit.addr, 0x1000);
        assert!(hit.wrapped);
        assert_eq!(session.cached_page_count(), 0);
    }

    #[test]
    fn memory_search_va_filter_limits_scanned_range() {
        let control = FakeMemoryBackend::new();
        control.add_region(
            0x1000,
            b"hit skip".to_vec(),
            MemoryPermissions::readable(),
            RegionFingerprint(1),
        );
        let mut session = MemorySession::open(Box::new(control)).unwrap();
        let query = MemorySearchQuery::parse("/hit/ in:va:0x1004-0x1008").unwrap();
        assert!(session
            .search(&query, Some(0x1000), MemorySearchDirection::Forward)
            .unwrap()
            .is_none());
    }

    #[test]
    fn freeze_is_depth_counted_and_thaw_resumes_once() {
        let (mut session, control) = session_with_region(vec![1, 2, 3, 4]);

        session.freeze().unwrap();
        session.freeze().unwrap();
        assert!(session.is_frozen());
        assert_eq!(session.freeze_depth(), 2);
        assert!(control.is_frozen());
        assert_eq!(control.freeze_count(), 1);

        session.thaw().unwrap();
        assert!(session.is_frozen());
        assert_eq!(session.freeze_depth(), 1);
        assert!(control.is_frozen());
        assert_eq!(control.thaw_count(), 0);

        session.thaw().unwrap();
        assert!(!session.is_frozen());
        assert_eq!(session.freeze_depth(), 0);
        assert!(!control.is_frozen());
        assert_eq!(control.thaw_count(), 1);
    }

    #[test]
    fn drop_auto_thaws_frozen_session() {
        let (mut session, control) = session_with_region(vec![1, 2, 3, 4]);
        session.freeze().unwrap();

        drop(session);

        assert!(!control.is_frozen());
        assert_eq!(control.thaw_count(), 1);
    }
}
