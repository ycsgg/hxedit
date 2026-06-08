use super::{MemoryBackend, ProcessInfo};

use crate::error::{HxError, HxResult};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

pub fn open_backend_for_pid(pid: u32) -> HxResult<Box<dyn MemoryBackend>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::ProcfsMemoryBackend::new(pid)))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsMemoryBackend::open(pid)?))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = pid;
        Err(HxError::MemoryUnavailable(format!(
            "process-memory backend is not implemented for {} yet",
            std::env::consts::OS
        )))
    }
}

pub fn open_backend_for_process(name: &str) -> HxResult<Box<dyn MemoryBackend>> {
    let matches = list_processes()?
        .into_iter()
        .filter(|process| process.name == name)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(HxError::MemoryUnavailable(format!(
            "no process named {name:?} was found"
        ))),
        [process] => open_backend_for_pid(process.pid),
        processes => Err(HxError::MemoryUnavailable(format!(
            "process name {name:?} is ambiguous ({} matches); use --pid",
            processes.len()
        ))),
    }
}

pub fn list_processes() -> HxResult<Vec<ProcessInfo>> {
    #[cfg(target_os = "linux")]
    {
        linux::list_processes()
    }
    #[cfg(target_os = "windows")]
    {
        windows::list_processes()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err(HxError::MemoryUnavailable(format!(
            "process listing is not implemented for {} yet",
            std::env::consts::OS
        )))
    }
}
