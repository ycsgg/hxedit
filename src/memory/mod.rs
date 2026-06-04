//! Process-memory model and backend abstractions.
//!
//! This module is intentionally feature-gated and self-contained.  The first
//! implementation phase builds the neutral model, fake backend, and lazy page
//! snapshots without changing file-mode editing behavior.

mod platform;
mod search;
mod session;
mod traits;
mod types;

#[cfg(test)]
mod fake;

pub use platform::{list_processes, open_backend_for_pid, open_backend_for_process};
pub use search::{MemoryRegionFilter, MemorySearchDirection, MemorySearchHit, MemorySearchQuery};
pub use session::{MemorySession, MemorySessionState, PAGE_SIZE};
pub use traits::MemoryBackend;
pub use types::{
    MemoryPermissions, MemoryRegion, ProcessFingerprint, ProcessInfo, RegionFingerprint, RegionKind,
};

#[cfg(test)]
pub(crate) use fake::FakeMemoryBackend;
