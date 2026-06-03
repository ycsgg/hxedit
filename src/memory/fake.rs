use std::sync::{Arc, Mutex};

use crate::error::{HxError, HxResult};

use super::{
    MemoryBackend, MemoryPermissions, MemoryRegion, ProcessFingerprint, ProcessInfo,
    RegionFingerprint, RegionKind,
};

#[derive(Debug, Default)]
struct FakeState {
    info: Option<ProcessInfo>,
    fingerprint: Option<ProcessFingerprint>,
    regions: Vec<FakeRegion>,
    read_count: usize,
    write_count: usize,
    freeze_count: usize,
    thaw_count: usize,
    frozen: bool,
}

#[derive(Debug, Clone)]
struct FakeRegion {
    meta: MemoryRegion,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FakeMemoryBackend {
    state: Arc<Mutex<FakeState>>,
}

impl FakeMemoryBackend {
    pub(crate) fn new() -> Self {
        let backend = Self::default();
        backend.set_process_info(ProcessInfo::new(4242, "fake"));
        backend.set_process_fingerprint(ProcessFingerprint(1));
        backend
    }

    pub(crate) fn add_region(
        &self,
        start: u64,
        bytes: Vec<u8>,
        permissions: MemoryPermissions,
        fingerprint: RegionFingerprint,
    ) -> MemoryRegion {
        let end = start + bytes.len() as u64;
        let meta = MemoryRegion::new(start, end, permissions, RegionKind::Anonymous, fingerprint);
        self.state.lock().unwrap().regions.push(FakeRegion {
            meta: meta.clone(),
            bytes,
        });
        meta
    }

    pub(crate) fn set_regions(&self, regions: Vec<MemoryRegion>) {
        let mut state = self.state.lock().unwrap();
        state.regions = regions
            .into_iter()
            .map(|meta| FakeRegion {
                bytes: vec![0; meta.len() as usize],
                meta,
            })
            .collect();
    }

    pub(crate) fn set_process_info(&self, info: ProcessInfo) {
        self.state.lock().unwrap().info = Some(info);
    }

    pub(crate) fn set_process_fingerprint(&self, fingerprint: ProcessFingerprint) {
        self.state.lock().unwrap().fingerprint = Some(fingerprint);
    }

    pub(crate) fn read_count(&self) -> usize {
        self.state.lock().unwrap().read_count
    }

    pub(crate) fn write_count(&self) -> usize {
        self.state.lock().unwrap().write_count
    }

    pub(crate) fn freeze_count(&self) -> usize {
        self.state.lock().unwrap().freeze_count
    }

    pub(crate) fn thaw_count(&self) -> usize {
        self.state.lock().unwrap().thaw_count
    }

    pub(crate) fn is_frozen(&self) -> bool {
        self.state.lock().unwrap().frozen
    }

    pub(crate) fn region_bytes(&self, start: u64) -> Option<Vec<u8>> {
        self.state
            .lock()
            .unwrap()
            .regions
            .iter()
            .find(|region| region.meta.start == start)
            .map(|region| region.bytes.clone())
    }
}

impl MemoryBackend for FakeMemoryBackend {
    fn list_processes(&mut self) -> HxResult<Vec<ProcessInfo>> {
        Ok(vec![self.process_info()?])
    }

    fn process_info(&mut self) -> HxResult<ProcessInfo> {
        self.state
            .lock()
            .unwrap()
            .info
            .clone()
            .ok_or_else(|| HxError::MemoryUnavailable("fake process info missing".to_owned()))
    }

    fn process_fingerprint(&mut self) -> HxResult<ProcessFingerprint> {
        self.state.lock().unwrap().fingerprint.ok_or_else(|| {
            HxError::MemoryUnavailable("fake process fingerprint missing".to_owned())
        })
    }

    fn memory_regions(&mut self) -> HxResult<Vec<MemoryRegion>> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .regions
            .iter()
            .map(|region| region.meta.clone())
            .collect())
    }

    fn read_at(&mut self, addr: u64, buf: &mut [u8]) -> HxResult<()> {
        let mut state = self.state.lock().unwrap();
        let Some(region) = state
            .regions
            .iter()
            .find(|region| region.meta.contains_range(addr, buf.len()))
        else {
            return Err(HxError::MemoryAccess {
                addr,
                len: buf.len(),
                message: "fake read out of range".to_owned(),
            });
        };
        if !region.meta.permissions.read {
            return Err(HxError::MemoryAccess {
                addr,
                len: buf.len(),
                message: "fake region is not readable".to_owned(),
            });
        }
        let start = (addr - region.meta.start) as usize;
        buf.copy_from_slice(&region.bytes[start..start + buf.len()]);
        state.read_count += 1;
        Ok(())
    }

    fn write_at(&mut self, addr: u64, data: &[u8]) -> HxResult<()> {
        let mut state = self.state.lock().unwrap();
        let Some(region) = state
            .regions
            .iter_mut()
            .find(|region| region.meta.contains_range(addr, data.len()))
        else {
            return Err(HxError::MemoryAccess {
                addr,
                len: data.len(),
                message: "fake write out of range".to_owned(),
            });
        };
        if !region.meta.permissions.write {
            return Err(HxError::MemoryAccess {
                addr,
                len: data.len(),
                message: "fake region is not writable".to_owned(),
            });
        }
        let start = (addr - region.meta.start) as usize;
        region.bytes[start..start + data.len()].copy_from_slice(data);
        state.write_count += 1;
        Ok(())
    }

    fn freeze(&mut self) -> HxResult<()> {
        let mut state = self.state.lock().unwrap();
        state.freeze_count += 1;
        state.frozen = true;
        Ok(())
    }

    fn thaw(&mut self) -> HxResult<()> {
        let mut state = self.state.lock().unwrap();
        state.thaw_count += 1;
        state.frozen = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_backend_reads_and_writes_region_bytes() {
        let control = FakeMemoryBackend::new();
        control.add_region(
            0x1000,
            vec![1, 2, 3, 4],
            MemoryPermissions::read_write(),
            RegionFingerprint(10),
        );
        let mut backend = control.clone();

        let mut bytes = [0; 2];
        backend.read_at(0x1001, &mut bytes).unwrap();
        assert_eq!(bytes, [2, 3]);

        backend.write_at(0x1002, &[9, 8]).unwrap();
        let mut bytes = [0; 4];
        backend.read_at(0x1000, &mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 9, 8]);
        assert_eq!(control.write_count(), 1);
    }
}
