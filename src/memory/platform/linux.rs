use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{HxError, HxResult};

use super::super::{
    MemoryBackend, MemoryPermissions, MemoryRegion, ProcessFingerprint, ProcessInfo,
    RegionFingerprint, RegionKind,
};

pub(crate) struct ProcfsMemoryBackend {
    pid: u32,
}

impl ProcfsMemoryBackend {
    pub(crate) const fn new(pid: u32) -> Self {
        Self { pid }
    }
}

impl MemoryBackend for ProcfsMemoryBackend {
    fn list_processes(&mut self) -> HxResult<Vec<ProcessInfo>> {
        list_processes()
    }

    fn process_info(&mut self) -> HxResult<ProcessInfo> {
        process_info_for_pid(self.pid)
    }

    fn process_fingerprint(&mut self) -> HxResult<ProcessFingerprint> {
        process_fingerprint_for_pid(self.pid)
    }

    fn memory_regions(&mut self) -> HxResult<Vec<MemoryRegion>> {
        memory_regions_for_pid(self.pid)
    }

    fn read_at(&mut self, addr: u64, buf: &mut [u8]) -> HxResult<()> {
        let file = File::open(proc_path(self.pid, "mem"))?;
        read_exact_at(&file, addr, buf).map_err(|err| HxError::MemoryAccess {
            addr,
            len: buf.len(),
            message: err.to_string(),
        })
    }

    fn write_at(&mut self, addr: u64, data: &[u8]) -> HxResult<()> {
        let file = OpenOptions::new()
            .write(true)
            .open(proc_path(self.pid, "mem"))?;
        write_all_at(&file, addr, data).map_err(|err| HxError::MemoryAccess {
            addr,
            len: data.len(),
            message: err.to_string(),
        })
    }

    fn freeze(&mut self) -> HxResult<()> {
        send_signal(self.pid, "-STOP")
    }

    fn thaw(&mut self) -> HxResult<()> {
        send_signal(self.pid, "-CONT")
    }
}

pub(crate) fn list_processes() -> HxResult<Vec<ProcessInfo>> {
    let mut processes = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if let Ok(info) = process_info_for_pid(pid) {
            processes.push(info);
        }
    }
    processes.sort_by_key(|process| process.pid);
    Ok(processes)
}

fn process_info_for_pid(pid: u32) -> HxResult<ProcessInfo> {
    fs::metadata(Path::new("/proc").join(pid.to_string()))?;
    let name = fs::read_to_string(proc_path(pid, "comm"))
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|_| format!("pid-{pid}"));
    let executable = fs::read_link(proc_path(pid, "exe")).ok();
    let mut info = ProcessInfo::new(pid, name);
    info.executable = executable;
    info.arch = Some(std::env::consts::ARCH.to_owned());
    Ok(info)
}

fn process_fingerprint_for_pid(pid: u32) -> HxResult<ProcessFingerprint> {
    let stat = fs::read_to_string(proc_path(pid, "stat"))?;
    let after_comm = stat
        .rsplit_once(") ")
        .map(|(_, rest)| rest)
        .ok_or_else(|| HxError::MemoryUnavailable(format!("invalid /proc/{pid}/stat")))?;
    let start_time = after_comm
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| HxError::MemoryUnavailable(format!("missing /proc/{pid}/stat starttime")))?
        .parse::<u128>()
        .map_err(|err| HxError::MemoryUnavailable(format!("invalid process starttime: {err}")))?;
    Ok(ProcessFingerprint(((pid as u128) << 64) | start_time))
}

fn memory_regions_for_pid(pid: u32) -> HxResult<Vec<MemoryRegion>> {
    let maps = fs::read_to_string(proc_path(pid, "maps"))?;
    let mut regions = Vec::new();
    for line in maps.lines() {
        if let Some(region) = parse_maps_line(line)? {
            regions.push(region);
        }
    }
    Ok(regions)
}

fn parse_maps_line(line: &str) -> HxResult<Option<MemoryRegion>> {
    let mut parts = line.split_whitespace();
    let Some(range) = parts.next() else {
        return Ok(None);
    };
    let Some(perms) = parts.next() else {
        return Ok(None);
    };
    let _offset = parts.next();
    let _dev = parts.next();
    let _inode = parts.next();
    let path_text = parts.collect::<Vec<_>>().join(" ");

    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| HxError::MemoryUnavailable(format!("invalid maps range: {range}")))?;
    let start = u64::from_str_radix(start, 16)
        .map_err(|err| HxError::MemoryUnavailable(format!("invalid region start: {err}")))?;
    let end = u64::from_str_radix(end, 16)
        .map_err(|err| HxError::MemoryUnavailable(format!("invalid region end: {err}")))?;
    let mut chars = perms.chars();
    let permissions = MemoryPermissions::new(
        matches!(chars.next(), Some('r')),
        matches!(chars.next(), Some('w')),
        matches!(chars.next(), Some('x')),
    );
    let path = (!path_text.is_empty()).then(|| PathBuf::from(path_text.clone()));
    let label = path_text
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(str::to_owned);
    let kind = classify_region(&path_text, perms);
    let fingerprint = fingerprint_region(start, end, permissions, kind, &path_text);
    Ok(Some(MemoryRegion {
        start,
        end,
        permissions,
        kind,
        path,
        label,
        fingerprint,
    }))
}

fn classify_region(path: &str, perms: &str) -> RegionKind {
    match path {
        "[heap]" => RegionKind::Heap,
        value if value.starts_with("[stack") => RegionKind::Stack,
        "[vdso]" => RegionKind::Vdso,
        "[vvar]" => RegionKind::Vvar,
        "[vsyscall]" => RegionKind::Vsyscall,
        "" => {
            if perms.contains('p') {
                RegionKind::Private
            } else if perms.contains('s') {
                RegionKind::Shared
            } else {
                RegionKind::Anonymous
            }
        }
        value if value.starts_with('[') => RegionKind::Unknown,
        _ => RegionKind::Mapped,
    }
}

fn fingerprint_region(
    start: u64,
    end: u64,
    permissions: MemoryPermissions,
    kind: RegionKind,
    path: &str,
) -> RegionFingerprint {
    let mut hasher = DefaultHasher::new();
    start.hash(&mut hasher);
    end.hash(&mut hasher);
    permissions.read.hash(&mut hasher);
    permissions.write.hash(&mut hasher);
    permissions.execute.hash(&mut hasher);
    kind.hash(&mut hasher);
    path.hash(&mut hasher);
    RegionFingerprint(hasher.finish())
}

fn proc_path(pid: u32, name: &str) -> PathBuf {
    Path::new("/proc").join(pid.to_string()).join(name)
}

fn send_signal(pid: u32, signal: &str) -> HxResult<()> {
    let output = Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    Err(HxError::MemoryUnavailable(format!(
        "failed to send {signal} to pid {pid}: {}",
        if message.is_empty() {
            output.status.to_string()
        } else {
            message.to_owned()
        }
    )))
}

fn read_exact_at(file: &File, mut addr: u64, mut buf: &mut [u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let read = file.read_at(buf, addr)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short process-memory read",
            ));
        }
        addr += read as u64;
        buf = &mut buf[read..];
    }
    Ok(())
}

fn write_all_at(file: &File, mut addr: u64, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let written = file.write_at(data, addr)?;
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short process-memory write",
            ));
        }
        addr += written as u64;
        data = &data[written..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procfs_backend_lists_current_process_and_reads_self_memory() {
        let pid = std::process::id();
        let processes = list_processes().unwrap();
        assert!(processes.iter().any(|process| process.pid == pid));

        let mut backend = ProcfsMemoryBackend::new(pid);
        assert_eq!(backend.process_info().unwrap().pid, pid);
        assert!(backend
            .memory_regions()
            .unwrap()
            .iter()
            .any(|region| region.permissions.read));

        let marker = 0x5a_u8;
        let addr = (&marker as *const u8) as u64;
        let mut out = [0_u8; 1];
        backend.read_at(addr, &mut out).unwrap();
        assert_eq!(out[0], marker);
    }
}
