use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionFingerprint(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessFingerprint(pub u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl MemoryPermissions {
    pub const fn new(read: bool, write: bool, execute: bool) -> Self {
        Self {
            read,
            write,
            execute,
        }
    }

    pub const fn readable() -> Self {
        Self::new(true, false, false)
    }

    pub const fn read_write() -> Self {
        Self::new(true, true, false)
    }

    pub const fn label(self) -> [char; 3] {
        [
            if self.read { 'r' } else { '-' },
            if self.write { 'w' } else { '-' },
            if self.execute { 'x' } else { '-' },
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionKind {
    Anonymous,
    Heap,
    Stack,
    Module,
    Mapped,
    Private,
    Shared,
    Vdso,
    Vsyscall,
    Vvar,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion {
    pub start: u64,
    pub end: u64,
    pub permissions: MemoryPermissions,
    pub kind: RegionKind,
    pub path: Option<PathBuf>,
    pub label: Option<String>,
    pub fingerprint: RegionFingerprint,
}

impl MemoryRegion {
    pub fn new(
        start: u64,
        end: u64,
        permissions: MemoryPermissions,
        kind: RegionKind,
        fingerprint: RegionFingerprint,
    ) -> Self {
        Self {
            start,
            end,
            permissions,
            kind,
            path: None,
            label: None,
            fingerprint,
        }
    }

    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn contains(&self, addr: u64) -> bool {
        self.start <= addr && addr < self.end
    }

    pub fn contains_range(&self, addr: u64, len: usize) -> bool {
        let Some(end) = addr.checked_add(len as u64) else {
            return false;
        };
        self.start <= addr && end <= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub arch: Option<String>,
}

impl ProcessInfo {
    pub fn new(pid: u32, name: impl Into<String>) -> Self {
        Self {
            pid,
            name: name.into(),
            executable: None,
            arch: None,
        }
    }
}
