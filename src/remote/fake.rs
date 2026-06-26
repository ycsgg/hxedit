use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};
use std::sync::{Mutex, OnceLock};

use crate::error::{HxError, HxResult};

use super::{RemoteFingerprint, RemoteSave, RemoteStat, RemoteTarget};

#[derive(Debug, Clone)]
struct FakeFile {
    bytes: Vec<u8>,
    mtime: u64,
    readonly: bool,
}

#[derive(Debug)]
pub(crate) struct FakeBackend {
    target: RemoteTarget,
    readonly: bool,
}

#[derive(Debug)]
struct FakeSave {
    target: RemoteTarget,
    expected: Option<RemoteFingerprint>,
    bytes: Vec<u8>,
}

static STORE: OnceLock<Mutex<HashMap<String, FakeFile>>> = OnceLock::new();

pub(crate) fn install_fake(uri: &str, bytes: &[u8], readonly: bool) -> RemoteTarget {
    let target = RemoteTarget::parse(uri).expect("valid fake remote URI");
    store().lock().unwrap().insert(
        target.key(),
        FakeFile {
            bytes: bytes.to_vec(),
            mtime: 1,
            readonly,
        },
    );
    target
}

pub(crate) fn fake_bytes(target: &RemoteTarget) -> Vec<u8> {
    store()
        .lock()
        .unwrap()
        .get(&target.key())
        .map(|file| file.bytes.clone())
        .expect("fake remote exists")
}

pub(crate) fn touch(target: &RemoteTarget, bytes: &[u8]) {
    let mut guard = store().lock().unwrap();
    let file = guard.get_mut(&target.key()).expect("fake remote exists");
    file.bytes = bytes.to_vec();
    file.mtime = file.mtime.saturating_add(1);
}

impl FakeBackend {
    pub(crate) fn open(target: RemoteTarget, readonly: bool) -> HxResult<Self> {
        if !store().lock().unwrap().contains_key(&target.key()) {
            return Err(HxError::Remote(format!(
                "fake remote target not found: {}",
                target.label()
            )));
        }
        Ok(Self { target, readonly })
    }

    pub(crate) fn read_at(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        let guard = store().lock().unwrap();
        let file = guard.get(&self.target.key()).ok_or_else(|| {
            HxError::Remote(format!("fake remote vanished: {}", self.target.label()))
        })?;
        let start = offset as usize;
        if start >= file.bytes.len() {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(len).min(file.bytes.len());
        Ok(file.bytes[start..end].to_vec())
    }

    pub(crate) fn begin_save(
        &self,
        expected: Option<RemoteFingerprint>,
    ) -> HxResult<Box<dyn RemoteSave>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        Ok(Box::new(FakeSave {
            target: self.target.clone(),
            expected,
            bytes: Vec::new(),
        }))
    }

    pub(crate) fn reload(&mut self) -> HxResult<RemoteStat> {
        let guard = store().lock().unwrap();
        let file = guard.get(&self.target.key()).ok_or_else(|| {
            HxError::Remote(format!("fake remote vanished: {}", self.target.label()))
        })?;
        Ok(stat_for(file, self.readonly || file.readonly))
    }
}

impl Write for FakeSave {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl RemoteSave for FakeSave {
    fn finish(self: Box<Self>) -> HxResult<RemoteStat> {
        let mut guard = store().lock().unwrap();
        let file = guard.get_mut(&self.target.key()).ok_or_else(|| {
            HxError::Remote(format!("fake remote vanished: {}", self.target.label()))
        })?;
        if Some(fingerprint_for(file)) != self.expected {
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }
        if file.readonly {
            return Err(HxError::ReadOnly);
        }
        file.bytes = self.bytes;
        file.mtime = file.mtime.saturating_add(1);
        Ok(stat_for(file, false))
    }
}

impl fmt::Display for FakeBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.target.label())
    }
}

fn store() -> &'static Mutex<HashMap<String, FakeFile>> {
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn stat_for(file: &FakeFile, readonly: bool) -> RemoteStat {
    RemoteStat {
        len: file.bytes.len() as u64,
        fingerprint: Some(fingerprint_for(file)),
        readonly,
        permissions: Some(0o644),
    }
}

fn fingerprint_for(file: &FakeFile) -> RemoteFingerprint {
    RemoteFingerprint {
        len: file.bytes.len() as u64,
        mtime: Some(file.mtime),
        file_id: None,
    }
}
