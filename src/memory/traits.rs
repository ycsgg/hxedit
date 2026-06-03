use crate::error::HxResult;

use super::{MemoryRegion, ProcessFingerprint, ProcessInfo};

pub trait MemoryBackend {
    fn list_processes(&mut self) -> HxResult<Vec<ProcessInfo>>;
    fn process_info(&mut self) -> HxResult<ProcessInfo>;
    fn process_fingerprint(&mut self) -> HxResult<ProcessFingerprint>;
    fn memory_regions(&mut self) -> HxResult<Vec<MemoryRegion>>;
    fn read_at(&mut self, addr: u64, buf: &mut [u8]) -> HxResult<()>;
    fn write_at(&mut self, addr: u64, data: &[u8]) -> HxResult<()>;
}
