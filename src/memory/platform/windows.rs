//! Windows process-memory backend.
//!
//! Mirrors the Linux procfs backend's behavior using Win32 APIs:
//! - read/write via `ReadProcessMemory` / `WriteProcessMemory`
//!   (with a temporary `VirtualProtectEx` when the target page is not writable)
//! - region enumeration via `VirtualQueryEx`
//! - process enumeration via the ToolHelp snapshot APIs
//! - process fingerprint via pid + `GetProcessTimes` creation time
//! - freeze/thaw via the **undocumented** `NtSuspendProcess` /
//!   `NtResumeProcess` ntdll exports (see `freeze` below)

use std::collections::hash_map::DefaultHasher;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, FILETIME, HANDLE, INVALID_HANDLE_VALUE, LUID,
};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_DEBUG_NAME,
    SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    VirtualProtectEx, VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, MEM_MAPPED,
    MEM_PRIVATE, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
    PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SUSPEND_RESUME, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

use crate::error::{HxError, HxResult};

use super::super::{
    MemoryBackend, MemoryPermissions, MemoryRegion, ProcessFingerprint, ProcessInfo,
    RegionFingerprint, RegionKind,
};

/// RAII wrapper around a process `HANDLE` so we always `CloseHandle`.
struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid handle returned by `OpenProcess` and is
        // only closed once (here, on drop).
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(crate) struct WindowsMemoryBackend {
    pid: u32,
}

impl WindowsMemoryBackend {
    pub(crate) fn open(pid: u32) -> HxResult<Self> {
        // Best-effort: enabling SeDebugPrivilege lets us touch processes we
        // own but were started elevated; failure is non-fatal because many
        // same-user targets are reachable without it.
        let _ = enable_debug_privilege();
        Ok(Self { pid })
    }

    fn open_process(&self, access: u32) -> HxResult<ProcessHandle> {
        // SAFETY: FFI call with a literal access mask and pid; the returned
        // handle is checked for null before use.
        let handle = unsafe { OpenProcess(access, 0, self.pid) };
        if handle.is_null() {
            return Err(HxError::MemoryUnavailable(format!(
                "failed to open process {}: {}",
                self.pid,
                last_error_message()
            )));
        }
        Ok(ProcessHandle(handle))
    }
}

impl MemoryBackend for WindowsMemoryBackend {
    fn list_processes(&mut self) -> HxResult<Vec<ProcessInfo>> {
        list_processes()
    }

    fn process_info(&mut self) -> HxResult<ProcessInfo> {
        process_info_for_pid(self.pid)
    }

    fn process_fingerprint(&mut self) -> HxResult<ProcessFingerprint> {
        let handle = self.open_process(PROCESS_QUERY_LIMITED_INFORMATION)?;
        let creation = process_creation_time(handle.raw()).ok_or_else(|| {
            HxError::MemoryUnavailable(format!(
                "failed to read process {} creation time: {}",
                self.pid,
                last_error_message()
            ))
        })?;
        Ok(ProcessFingerprint(
            ((self.pid as u128) << 64) | creation as u128,
        ))
    }

    fn memory_regions(&mut self) -> HxResult<Vec<MemoryRegion>> {
        let handle = self.open_process(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ)?;
        memory_regions(handle.raw())
    }

    fn read_at(&mut self, addr: u64, buf: &mut [u8]) -> HxResult<()> {
        let handle = self.open_process(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION)?;
        let mut read = 0usize;
        // SAFETY: `buf` is a valid mutable slice; `addr` is treated as the
        // remote address space pointer and never dereferenced locally.
        let ok = unsafe {
            ReadProcessMemory(
                handle.raw(),
                addr as *const c_void,
                buf.as_mut_ptr().cast::<c_void>(),
                buf.len(),
                &mut read,
            )
        };
        if ok == 0 || read != buf.len() {
            return Err(HxError::MemoryAccess {
                addr,
                len: buf.len(),
                message: last_error_message(),
            });
        }
        Ok(())
    }

    fn write_at(&mut self, addr: u64, data: &[u8]) -> HxResult<()> {
        let handle =
            self.open_process(PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_INFORMATION)?;

        // Temporarily make the target pages writable if they are not already,
        // then restore the original protection afterwards.
        let mut old_protect = 0u32;
        // SAFETY: FFI call; `addr`/`data.len()` describe the remote range only.
        let relaxed = unsafe {
            VirtualProtectEx(
                handle.raw(),
                addr as *const c_void,
                data.len(),
                PAGE_READWRITE,
                &mut old_protect,
            )
        } != 0;

        let mut written = 0usize;
        // SAFETY: `data` is a valid slice; remote address is not dereferenced
        // locally.
        let ok = unsafe {
            WriteProcessMemory(
                handle.raw(),
                addr as *const c_void,
                data.as_ptr().cast::<c_void>(),
                data.len(),
                &mut written,
            )
        };
        let write_failed = ok == 0 || written != data.len();
        let message = if write_failed {
            Some(last_error_message())
        } else {
            None
        };

        if relaxed {
            let mut restored = 0u32;
            // SAFETY: restoring the protection we just changed.
            unsafe {
                VirtualProtectEx(
                    handle.raw(),
                    addr as *const c_void,
                    data.len(),
                    old_protect,
                    &mut restored,
                );
            }
        }

        if let Some(message) = message {
            return Err(HxError::MemoryAccess {
                addr,
                len: data.len(),
                message,
            });
        }
        Ok(())
    }

    fn freeze(&mut self) -> HxResult<()> {
        let handle = self.open_process(PROCESS_SUSPEND_RESUME)?;
        nt_suspend_resume(handle.raw(), NtControl::Suspend).map_err(|message| {
            HxError::MemoryUnavailable(format!("failed to suspend process {}: {message}", self.pid))
        })
    }

    fn thaw(&mut self) -> HxResult<()> {
        let handle = self.open_process(PROCESS_SUSPEND_RESUME)?;
        nt_suspend_resume(handle.raw(), NtControl::Resume).map_err(|message| {
            HxError::MemoryUnavailable(format!("failed to resume process {}: {message}", self.pid))
        })
    }
}

pub(crate) fn list_processes() -> HxResult<Vec<ProcessInfo>> {
    // SAFETY: FFI call with a literal flag and pid 0 (whole-system snapshot).
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(HxError::MemoryUnavailable(format!(
            "failed to snapshot processes: {}",
            last_error_message()
        )));
    }
    let snapshot = ProcessHandle(snapshot);

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        // SAFETY: zero-initialising the remaining POD fields is valid.
        ..unsafe { std::mem::zeroed() }
    };

    let mut processes = Vec::new();
    // SAFETY: `entry` is a valid, sized PROCESSENTRY32W.
    let mut ok = unsafe { Process32FirstW(snapshot.raw(), &mut entry) } != 0;
    while ok {
        let name = wide_to_string(&entry.szExeFile);
        let mut info = ProcessInfo::new(entry.th32ProcessID, name);
        info.executable = process_image_path(entry.th32ProcessID);
        info.arch = Some(std::env::consts::ARCH.to_owned());
        processes.push(info);
        // SAFETY: iterating the snapshot we created above.
        ok = unsafe { Process32NextW(snapshot.raw(), &mut entry) } != 0;
    }

    processes.sort_by_key(|process| process.pid);
    Ok(processes)
}

fn process_info_for_pid(pid: u32) -> HxResult<ProcessInfo> {
    let name = process_info_name(pid).unwrap_or_else(|| format!("pid-{pid}"));
    let mut info = ProcessInfo::new(pid, name);
    info.executable = process_image_path(pid);
    info.arch = Some(std::env::consts::ARCH.to_owned());
    Ok(info)
}

/// Look up a process name by scanning the ToolHelp snapshot for `pid`.
fn process_info_name(pid: u32) -> Option<String> {
    list_processes()
        .ok()?
        .into_iter()
        .find(|process| process.pid == pid)
        .map(|process| process.name)
}

fn process_image_path(pid: u32) -> Option<PathBuf> {
    // SAFETY: FFI call; null handle is checked before use.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let handle = ProcessHandle(handle);

    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    // SAFETY: `buffer`/`size` describe a valid, sized UTF-16 output buffer.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle.raw(),
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut size,
        )
    };
    if ok == 0 {
        return None;
    }
    buffer.truncate(size as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

fn process_creation_time(handle: HANDLE) -> Option<u64> {
    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    // SAFETY: all four FILETIME out-params are valid local storage.
    let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }
    Some(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

fn memory_regions(handle: HANDLE) -> HxResult<Vec<MemoryRegion>> {
    let mut regions = Vec::new();
    let mut addr: u64 = 0;
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: `info` is valid, sized storage; `addr` is a remote address.
        let written = unsafe {
            VirtualQueryEx(
                handle,
                addr as *const c_void,
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if written == 0 {
            break;
        }

        let base = info.BaseAddress as u64;
        let size = info.RegionSize as u64;
        let Some(end) = base.checked_add(size) else {
            break;
        };

        if info.State == MEM_COMMIT {
            let permissions = permissions_from_protect(info.Protect);
            let kind = classify_region(info.Type);
            let fingerprint = fingerprint_region(base, end, permissions, kind);
            regions.push(MemoryRegion {
                start: base,
                end,
                permissions,
                kind,
                path: None,
                label: None,
                fingerprint,
            });
        }

        // Advance to the next region; stop if the address space wraps.
        match addr.checked_add(size.max(1)) {
            Some(next) if next > addr => addr = next,
            _ => break,
        }
    }
    Ok(regions)
}

fn permissions_from_protect(protect: u32) -> MemoryPermissions {
    // PAGE_GUARD / other flags may be OR-ed in; mask to the base protection.
    let base = protect & !PAGE_GUARD;
    let (read, write, execute) = match base {
        PAGE_NOACCESS => (false, false, false),
        PAGE_READONLY => (true, false, false),
        PAGE_READWRITE | PAGE_WRITECOPY => (true, true, false),
        PAGE_EXECUTE => (false, false, true),
        PAGE_EXECUTE_READ => (true, false, true),
        PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY => (true, true, true),
        _ => (true, false, false),
    };
    MemoryPermissions::new(read, write, execute)
}

fn classify_region(region_type: u32) -> RegionKind {
    match region_type {
        MEM_IMAGE => RegionKind::Module,
        MEM_MAPPED => RegionKind::Mapped,
        MEM_PRIVATE => RegionKind::Private,
        _ => RegionKind::Unknown,
    }
}

fn fingerprint_region(
    start: u64,
    end: u64,
    permissions: MemoryPermissions,
    kind: RegionKind,
) -> RegionFingerprint {
    let mut hasher = DefaultHasher::new();
    start.hash(&mut hasher);
    end.hash(&mut hasher);
    permissions.read.hash(&mut hasher);
    permissions.write.hash(&mut hasher);
    permissions.execute.hash(&mut hasher);
    kind.hash(&mut hasher);
    RegionFingerprint(hasher.finish())
}

/// Best-effort: request `SeDebugPrivilege` for the current process token.
fn enable_debug_privilege() -> HxResult<()> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is valid storage.
    let ok = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    };
    if ok == 0 {
        return Err(HxError::MemoryUnavailable(format!(
            "failed to open process token: {}",
            last_error_message()
        )));
    }
    let token = ProcessHandle(token);

    let mut luid = LUID {
        LowPart: 0,
        HighPart: 0,
    };
    // SAFETY: SE_DEBUG_NAME is a static PCWSTR; `luid` is valid storage.
    let ok = unsafe { LookupPrivilegeValueW(std::ptr::null(), SE_DEBUG_NAME, &mut luid) };
    if ok == 0 {
        return Err(HxError::MemoryUnavailable(format!(
            "failed to look up SeDebugPrivilege: {}",
            last_error_message()
        )));
    }

    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: `privileges` is a valid single-entry TOKEN_PRIVILEGES.
    let ok = unsafe {
        AdjustTokenPrivileges(
            token.raw(),
            0,
            &privileges,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(HxError::MemoryUnavailable(format!(
            "failed to adjust token privileges: {}",
            last_error_message()
        )));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum NtControl {
    Suspend,
    Resume,
}

/// Suspend or resume an entire process via ntdll.
///
/// `NtSuspendProcess` / `NtResumeProcess` are **undocumented** ntdll exports:
/// Windows ships no official "suspend whole process" API. They have been
/// stable for many years and are the cleanest analogue to Linux's
/// `SIGSTOP`/`SIGCONT`, but Microsoft does not guarantee them, so we resolve
/// them dynamically via `GetProcAddress` and degrade gracefully if missing.
fn nt_suspend_resume(handle: HANDLE, control: NtControl) -> Result<(), String> {
    type NtProcessFn = unsafe extern "system" fn(HANDLE) -> i32;

    let proc_name: &[u8] = match control {
        NtControl::Suspend => b"NtSuspendProcess\0",
        NtControl::Resume => b"NtResumeProcess\0",
    };

    // UTF-16 "ntdll.dll\0" for GetModuleHandleW.
    let module: Vec<u16> = "ntdll.dll\0".encode_utf16().collect();
    // SAFETY: `module` is a valid null-terminated wide string.
    let ntdll = unsafe { GetModuleHandleW(module.as_ptr()) };
    if ntdll.is_null() {
        return Err(format!("ntdll.dll not loaded: {}", last_error_message()));
    }

    // SAFETY: `proc_name` is a null-terminated ASCII string; `ntdll` is valid.
    let address = unsafe { GetProcAddress(ntdll, proc_name.as_ptr()) };
    let Some(address) = address else {
        return Err("ntdll export not found (unsupported Windows build)".to_owned());
    };

    // SAFETY: the resolved export matches the `NtProcessFn` ABI; `handle` is a
    // valid process handle opened with PROCESS_SUSPEND_RESUME.
    let func: NtProcessFn = unsafe { std::mem::transmute::<_, NtProcessFn>(address) };
    let status = unsafe { func(handle) };
    if status < 0 {
        return Err(format!("NTSTATUS 0x{status:08x}"));
    }
    Ok(())
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

fn last_error_message() -> String {
    // SAFETY: GetLastError has no preconditions.
    let code = unsafe { GetLastError() };
    format!("os error {code}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_backend_lists_current_process_and_reads_self_memory() {
        let pid = std::process::id();
        let processes = list_processes().unwrap();
        assert!(processes.iter().any(|process| process.pid == pid));

        let mut backend = WindowsMemoryBackend::open(pid).unwrap();
        assert_eq!(backend.process_info().unwrap().pid, pid);
        assert!(backend
            .memory_regions()
            .unwrap()
            .iter()
            .any(|region| region.permissions.read));

        // Reading our own memory needs no extra privilege.
        let marker = 0x5a_u8;
        let addr = std::ptr::from_ref(&marker) as u64;
        let mut out = [0_u8; 1];
        backend.read_at(addr, &mut out).unwrap();
        assert_eq!(out[0], marker);
    }
}
