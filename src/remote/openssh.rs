use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{HxError, HxResult};

use super::{RemoteFingerprint, RemoteSave, RemoteStat, RemoteTarget};

const SFTP_VERSION: u32 = 3;
const MAX_PACKET_LEN: usize = 16 * 1024 * 1024;
const READ_CHUNK: usize = 256 * 1024;
const READ_WINDOW: usize = 64;
const WRITE_CHUNK: usize = 32 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

const SSH_FXP_INIT: u8 = 1;
const SSH_FXP_VERSION: u8 = 2;
const SSH_FXP_OPEN: u8 = 3;
const SSH_FXP_CLOSE: u8 = 4;
const SSH_FXP_READ: u8 = 5;
const SSH_FXP_WRITE: u8 = 6;
const SSH_FXP_SETSTAT: u8 = 9;
const SSH_FXP_REMOVE: u8 = 13;
const SSH_FXP_STAT: u8 = 17;
const SSH_FXP_STATUS: u8 = 101;
const SSH_FXP_HANDLE: u8 = 102;
const SSH_FXP_DATA: u8 = 103;
const SSH_FXP_ATTRS: u8 = 105;
const SSH_FXP_EXTENDED: u8 = 200;

const SSH_FXF_READ: u32 = 0x0000_0001;
const SSH_FXF_WRITE: u32 = 0x0000_0002;
const SSH_FXF_CREAT: u32 = 0x0000_0008;
const SSH_FXF_TRUNC: u32 = 0x0000_0010;

const SSH_FILEXFER_ATTR_SIZE: u32 = 0x0000_0001;
const SSH_FILEXFER_ATTR_UIDGID: u32 = 0x0000_0002;
const SSH_FILEXFER_ATTR_PERMISSIONS: u32 = 0x0000_0004;
const SSH_FILEXFER_ATTR_ACMODTIME: u32 = 0x0000_0008;
const SSH_FILEXFER_ATTR_EXTENDED: u32 = 0x8000_0000;

const SSH_FX_OK: u32 = 0;
const SSH_FX_EOF: u32 = 1;

pub(crate) struct OpenSshSftpBackend {
    target: RemoteTarget,
    readonly: bool,
    client: SftpClient,
    handle: Vec<u8>,
}

struct OpenSshSftpSave {
    target: RemoteTarget,
    expected: Option<RemoteFingerprint>,
    permissions: Option<u32>,
    client: SftpClient,
    handle: Option<Vec<u8>>,
    offset: u64,
    temp_path: String,
}

struct SftpClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u32,
    stderr: Arc<Mutex<Vec<u8>>>,
    extensions: HashSet<String>,
}

struct ResponsePacket {
    id: u32,
    typ: u8,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
struct SftpAttrs {
    size: Option<u64>,
    perm: Option<u32>,
    mtime: Option<u64>,
}

#[derive(Debug)]
struct SftpStatus {
    code: u32,
    message: String,
}

struct PacketReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl fmt::Debug for OpenSshSftpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenSshSftpBackend")
            .field("target", &self.target.label())
            .field("readonly", &self.readonly)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for OpenSshSftpSave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenSshSftpSave")
            .field("target", &self.target.label())
            .field("temp_path", &self.temp_path)
            .finish_non_exhaustive()
    }
}

impl OpenSshSftpBackend {
    pub(crate) fn open(target: RemoteTarget, readonly: bool) -> HxResult<Self> {
        let mut client = SftpClient::connect(&target)?;
        let handle = client.open_path(target.path(), SSH_FXF_READ)?;
        Ok(Self {
            target,
            readonly,
            client,
            handle,
        })
    }

    pub(crate) fn read_at(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        self.client.read_at(&self.handle, offset, len)
    }

    pub(crate) fn begin_save(
        &self,
        expected: Option<RemoteFingerprint>,
    ) -> HxResult<Box<dyn RemoteSave>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }

        let mut client = SftpClient::connect(&self.target)?;
        let current = stat_with(&mut client, &self.target, self.readonly)?;
        if current.fingerprint != expected {
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        let temp_path = temp_path_for(self.target.path());
        let handle = client.open_path(&temp_path, SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC)?;
        Ok(Box::new(OpenSshSftpSave {
            target: self.target.clone(),
            expected,
            permissions: current.permissions,
            client,
            handle: Some(handle),
            offset: 0,
            temp_path,
        }))
    }

    pub(crate) fn reload(&mut self) -> HxResult<RemoteStat> {
        let stat = stat_with(&mut self.client, &self.target, self.readonly)?;
        let _ = self.client.close_handle(&self.handle);
        self.handle = self.client.open_path(self.target.path(), SSH_FXF_READ)?;
        Ok(stat)
    }
}

impl Write for OpenSshSftpSave {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| io::Error::other("remote temp file is already closed"))?;
        self.client
            .write_all_at(handle, self.offset, buf)
            .map_err(|err| io::Error::other(err.to_string()))?;
        self.offset += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl RemoteSave for OpenSshSftpSave {
    fn finish(mut self: Box<Self>) -> HxResult<RemoteStat> {
        if let Some(handle) = self.handle.take() {
            self.client.close_handle(&handle)?;
        }

        let current = stat_with(&mut self.client, &self.target, false)?;
        if current.fingerprint != self.expected {
            let _ = self.client.remove_path(&self.temp_path);
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        if let Some(permissions) = self.permissions {
            let _ = self.client.set_permissions(&self.temp_path, permissions);
        }

        self.client
            .posix_rename(&self.temp_path, self.target.path())
            .inspect_err(|_err| {
                let _ = self.client.remove_path(&self.temp_path);
            })?;

        stat_with(&mut self.client, &self.target, false)
    }
}

impl SftpClient {
    fn connect(target: &RemoteTarget) -> HxResult<Self> {
        let mut child = ssh_command(target)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| HxError::Remote(format!("failed to start ssh: {err}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HxError::Remote("failed to open ssh stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HxError::Remote("failed to open ssh stdout".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| HxError::Remote("failed to open ssh stderr".to_owned()))?;
        let stderr = drain_stderr(stderr);

        let mut client = Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            next_id: 1,
            stderr,
            extensions: HashSet::new(),
        };
        client.init()?;
        Ok(client)
    }

    fn init(&mut self) -> HxResult<()> {
        let mut payload = Vec::new();
        put_u32(&mut payload, SFTP_VERSION);
        self.write_packet(SSH_FXP_INIT, &payload, true)?;

        let (typ, payload) = self.read_packet()?;
        if typ != SSH_FXP_VERSION {
            return Err(HxError::Remote(format!(
                "unexpected SFTP init response packet type {typ}"
            )));
        }
        let mut reader = PacketReader::new(&payload);
        let version = reader.read_u32()?;
        if version != SFTP_VERSION {
            return Err(HxError::Remote(format!(
                "unsupported SFTP protocol version {version}; expected {SFTP_VERSION}"
            )));
        }
        while !reader.is_empty() {
            let name = reader.read_string_lossy()?;
            let _data = reader.read_string()?;
            self.extensions.insert(name);
        }
        Ok(())
    }

    fn open_path(&mut self, path: &str, pflags: u32) -> HxResult<Vec<u8>> {
        let id = self.next_request_id();
        let mut payload = request_payload(id);
        put_string(&mut payload, path.as_bytes());
        put_u32(&mut payload, pflags);
        put_empty_attrs(&mut payload);
        self.write_packet(SSH_FXP_OPEN, &payload, true)?;

        let response = self.read_response()?;
        self.ensure_response_id(response.id, id)?;
        match response.typ {
            SSH_FXP_HANDLE => {
                let mut reader = PacketReader::new(&response.payload);
                reader.read_string().map(|bytes| bytes.to_vec())
            }
            SSH_FXP_STATUS => Err(status_to_error(
                parse_status(&response.payload)?,
                format!("open {path}"),
            )),
            typ => Err(unexpected_response("open", typ)),
        }
    }

    fn read_at(&mut self, handle: &[u8], offset: u64, len: usize) -> HxResult<Vec<u8>> {
        let mut out = vec![0; len];
        let mut next = 0usize;
        let mut pending: HashMap<u32, (usize, usize)> = HashMap::new();
        let mut retry = VecDeque::new();
        let mut eof_at: Option<usize> = None;

        while next < len || !retry.is_empty() || !pending.is_empty() {
            while eof_at.is_none() && next < len && pending.len() < READ_WINDOW {
                let request_len = READ_CHUNK.min(len - next);
                let id = self.send_read_request(handle, offset + next as u64, request_len)?;
                pending.insert(id, (next, request_len));
                next += request_len;
            }
            while eof_at.is_none() && !retry.is_empty() && pending.len() < READ_WINDOW {
                let (start, request_len) = retry
                    .pop_front()
                    .expect("retry queue was checked for emptiness");
                let id = self.send_read_request(handle, offset + start as u64, request_len)?;
                pending.insert(id, (start, request_len));
            }
            self.flush()?;

            let response = self.read_response()?;
            let Some((start, requested)) = pending.remove(&response.id) else {
                return Err(HxError::Remote(format!(
                    "unexpected SFTP response id {}",
                    response.id
                )));
            };

            match response.typ {
                SSH_FXP_DATA => {
                    let mut reader = PacketReader::new(&response.payload);
                    let data = reader.read_string()?;
                    let end = start.saturating_add(data.len()).min(out.len());
                    out[start..end].copy_from_slice(&data[..end - start]);
                    if data.is_empty() {
                        eof_at = Some(start);
                    } else if data.len() < requested {
                        retry.push_back((start + data.len(), requested - data.len()));
                    }
                }
                SSH_FXP_STATUS => {
                    let status = parse_status(&response.payload)?;
                    if status.code == SSH_FX_EOF {
                        eof_at = Some(start);
                    } else {
                        return Err(status_to_error(status, "read".to_owned()));
                    }
                }
                typ => return Err(unexpected_response("read", typ)),
            }
        }

        if let Some(end) = eof_at {
            out.truncate(end);
        }
        Ok(out)
    }

    fn send_read_request(&mut self, handle: &[u8], offset: u64, len: usize) -> HxResult<u32> {
        let id = self.next_request_id();
        let mut payload = request_payload(id);
        put_string(&mut payload, handle);
        put_u64(&mut payload, offset);
        put_u32(&mut payload, len as u32);
        self.write_packet(SSH_FXP_READ, &payload, false)?;
        Ok(id)
    }

    fn write_all_at(&mut self, handle: &[u8], mut offset: u64, mut buf: &[u8]) -> HxResult<()> {
        while !buf.is_empty() {
            let count = WRITE_CHUNK.min(buf.len());
            let chunk = &buf[..count];
            let id = self.next_request_id();
            let mut payload = request_payload(id);
            put_string(&mut payload, handle);
            put_u64(&mut payload, offset);
            put_string(&mut payload, chunk);
            self.write_packet(SSH_FXP_WRITE, &payload, true)?;
            self.expect_status(id, "write")?;
            offset += count as u64;
            buf = &buf[count..];
        }
        Ok(())
    }

    fn close_handle(&mut self, handle: &[u8]) -> HxResult<()> {
        let id = self.next_request_id();
        let mut payload = request_payload(id);
        put_string(&mut payload, handle);
        self.write_packet(SSH_FXP_CLOSE, &payload, true)?;
        self.expect_status(id, "close")
    }

    fn stat_path(&mut self, path: &str) -> HxResult<SftpAttrs> {
        let id = self.next_request_id();
        let mut payload = request_payload(id);
        put_string(&mut payload, path.as_bytes());
        self.write_packet(SSH_FXP_STAT, &payload, true)?;

        let response = self.read_response()?;
        self.ensure_response_id(response.id, id)?;
        match response.typ {
            SSH_FXP_ATTRS => parse_attrs(&response.payload),
            SSH_FXP_STATUS => Err(status_to_error(
                parse_status(&response.payload)?,
                format!("stat {path}"),
            )),
            typ => Err(unexpected_response("stat", typ)),
        }
    }

    fn set_permissions(&mut self, path: &str, permissions: u32) -> HxResult<()> {
        let id = self.next_request_id();
        let mut payload = request_payload(id);
        put_string(&mut payload, path.as_bytes());
        put_permissions_attrs(&mut payload, permissions);
        self.write_packet(SSH_FXP_SETSTAT, &payload, true)?;
        self.expect_status(id, "setstat")
    }

    fn remove_path(&mut self, path: &str) -> HxResult<()> {
        let id = self.next_request_id();
        let mut payload = request_payload(id);
        put_string(&mut payload, path.as_bytes());
        self.write_packet(SSH_FXP_REMOVE, &payload, true)?;
        self.expect_status(id, "remove")
    }

    fn posix_rename(&mut self, old_path: &str, new_path: &str) -> HxResult<()> {
        if !self.extensions.contains("posix-rename@openssh.com") {
            return Err(HxError::Remote(
                "SFTP server does not advertise posix-rename@openssh.com; refusing non-atomic overwrite save"
                    .to_owned(),
            ));
        }
        let id = self.next_request_id();
        let mut payload = request_payload(id);
        put_string(&mut payload, b"posix-rename@openssh.com");
        put_string(&mut payload, old_path.as_bytes());
        put_string(&mut payload, new_path.as_bytes());
        self.write_packet(SSH_FXP_EXTENDED, &payload, true)?;
        self.expect_status(id, "posix-rename")
    }

    fn expect_status(&mut self, expected_id: u32, operation: &str) -> HxResult<()> {
        let response = self.read_response()?;
        self.ensure_response_id(response.id, expected_id)?;
        if response.typ != SSH_FXP_STATUS {
            return Err(unexpected_response(operation, response.typ));
        }
        let status = parse_status(&response.payload)?;
        if status.code == SSH_FX_OK {
            Ok(())
        } else {
            Err(status_to_error(status, operation.to_owned()))
        }
    }

    fn ensure_response_id(&self, actual: u32, expected: u32) -> HxResult<()> {
        if actual == expected {
            Ok(())
        } else {
            Err(HxError::Remote(format!(
                "unexpected SFTP response id {actual}; expected {expected}"
            )))
        }
    }

    fn next_request_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    fn write_packet(&mut self, typ: u8, payload: &[u8], flush: bool) -> HxResult<()> {
        let len = 1usize
            .checked_add(payload.len())
            .ok_or_else(|| HxError::Remote("SFTP packet too large".to_owned()))?;
        let len =
            u32::try_from(len).map_err(|_| HxError::Remote("SFTP packet too large".to_owned()))?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| HxError::Remote("ssh stdin is closed".to_owned()))?;
        stdin.write_all(&len.to_be_bytes()).map_err(remote_io)?;
        stdin.write_all(&[typ]).map_err(remote_io)?;
        stdin.write_all(payload).map_err(remote_io)?;
        if flush {
            stdin.flush().map_err(remote_io)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> HxResult<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| HxError::Remote("ssh stdin is closed".to_owned()))?;
        stdin.flush().map_err(remote_io)
    }

    fn read_response(&mut self) -> HxResult<ResponsePacket> {
        let (typ, payload) = self.read_packet()?;
        let mut reader = PacketReader::new(&payload);
        let id = reader.read_u32()?;
        Ok(ResponsePacket {
            id,
            typ,
            payload: reader.remaining().to_vec(),
        })
    }

    fn read_packet(&mut self) -> HxResult<(u8, Vec<u8>)> {
        let mut len_buf = [0u8; 4];
        self.stdout
            .read_exact(&mut len_buf)
            .map_err(|err| self.read_error(err))?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 || len > MAX_PACKET_LEN {
            return Err(HxError::Remote(format!("invalid SFTP packet length {len}")));
        }
        let mut body = vec![0; len];
        self.stdout
            .read_exact(&mut body)
            .map_err(|err| self.read_error(err))?;
        let typ = body[0];
        Ok((typ, body[1..].to_vec()))
    }

    fn read_error(&self, err: io::Error) -> HxError {
        let mut message = format!("ssh transport read failed: {err}");
        let stderr = self.stderr_message();
        if !stderr.is_empty() {
            message.push_str(": ");
            message.push_str(&stderr);
        }
        HxError::Remote(message)
    }

    fn stderr_message(&self) -> String {
        let Ok(stderr) = self.stderr.lock() else {
            return String::new();
        };
        String::from_utf8_lossy(&stderr).trim().to_owned()
    }
}

impl Drop for SftpClient {
    fn drop(&mut self) {
        let _ = self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl PacketReader<'_> {
    fn new(bytes: &[u8]) -> PacketReader<'_> {
        PacketReader { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset >= self.bytes.len()
    }

    fn remaining(&self) -> &[u8] {
        &self.bytes[self.offset..]
    }

    fn read_u32(&mut self) -> HxResult<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes(
            bytes.try_into().expect("slice length was checked"),
        ))
    }

    fn read_u64(&mut self) -> HxResult<u64> {
        let bytes = self.take(8)?;
        Ok(u64::from_be_bytes(
            bytes.try_into().expect("slice length was checked"),
        ))
    }

    fn read_string(&mut self) -> HxResult<&[u8]> {
        let len = self.read_u32()? as usize;
        self.take(len)
    }

    fn read_string_lossy(&mut self) -> HxResult<String> {
        let bytes = self.read_string()?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn take(&mut self, len: usize) -> HxResult<&[u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| HxError::Remote("malformed SFTP packet".to_owned()))?;
        if end > self.bytes.len() {
            return Err(HxError::Remote("truncated SFTP packet".to_owned()));
        }
        let out = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(out)
    }
}

fn ssh_command(target: &RemoteTarget) -> Command {
    let mut command = Command::new("ssh");
    command
        .arg("-T")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ClearAllForwardings=yes");
    if env::var_os("HXEDIT_SFTP_INSECURE").is_some() {
        command
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg("UserKnownHostsFile=/dev/null");
    }
    if let Some(port) = target.port() {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(username) = target.username() {
        command.arg("-l").arg(username);
    }
    command.arg("-s").arg("--").arg(target.host()).arg("sftp");
    command
}

fn drain_stderr(mut stderr: ChildStderr) -> Arc<Mutex<Vec<u8>>> {
    let output = Arc::new(Mutex::new(Vec::new()));
    let target = Arc::clone(&output);
    let _ = thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            let Ok(read) = stderr.read(&mut buf) else {
                break;
            };
            if read == 0 {
                break;
            }
            let Ok(mut output) = target.lock() else {
                break;
            };
            output.extend_from_slice(&buf[..read]);
            if output.len() > STDERR_LIMIT {
                let drain = output.len() - STDERR_LIMIT;
                output.drain(..drain);
            }
        }
    });
    output
}

fn stat_with(
    client: &mut SftpClient,
    target: &RemoteTarget,
    readonly: bool,
) -> HxResult<RemoteStat> {
    let stat = client.stat_path(target.path())?;
    let len = stat
        .size
        .ok_or_else(|| HxError::Remote(format!("remote target has no size: {}", target.label())))?;
    Ok(RemoteStat {
        len,
        fingerprint: Some(RemoteFingerprint {
            len,
            mtime: stat.mtime,
            file_id: stat.perm.map(|perm| format!("perm:{perm:o}")),
        }),
        readonly,
        permissions: stat.perm,
    })
}

fn temp_path_for(path: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    match path.rsplit_once('/') {
        Some((dir, name)) if !name.is_empty() => {
            format!("{dir}/.{name}.hxedit.tmp.{stamp}")
        }
        _ => format!(".hxedit.tmp.{stamp}"),
    }
}

fn request_payload(id: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    put_u32(&mut payload, id);
    payload
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_string(out: &mut Vec<u8>, value: &[u8]) {
    put_u32(out, value.len() as u32);
    out.extend_from_slice(value);
}

fn put_empty_attrs(out: &mut Vec<u8>) {
    put_u32(out, 0);
}

fn put_permissions_attrs(out: &mut Vec<u8>, permissions: u32) {
    put_u32(out, SSH_FILEXFER_ATTR_PERMISSIONS);
    put_u32(out, permissions);
}

fn parse_attrs(payload: &[u8]) -> HxResult<SftpAttrs> {
    let mut reader = PacketReader::new(payload);
    let flags = reader.read_u32()?;
    let mut attrs = SftpAttrs::default();
    if flags & SSH_FILEXFER_ATTR_SIZE != 0 {
        attrs.size = Some(reader.read_u64()?);
    }
    if flags & SSH_FILEXFER_ATTR_UIDGID != 0 {
        let _uid = reader.read_u32()?;
        let _gid = reader.read_u32()?;
    }
    if flags & SSH_FILEXFER_ATTR_PERMISSIONS != 0 {
        attrs.perm = Some(reader.read_u32()?);
    }
    if flags & SSH_FILEXFER_ATTR_ACMODTIME != 0 {
        let _atime = reader.read_u32()?;
        attrs.mtime = Some(reader.read_u32()? as u64);
    }
    if flags & SSH_FILEXFER_ATTR_EXTENDED != 0 {
        let count = reader.read_u32()?;
        for _ in 0..count {
            let _type = reader.read_string()?;
            let _data = reader.read_string()?;
        }
    }
    Ok(attrs)
}

fn parse_status(payload: &[u8]) -> HxResult<SftpStatus> {
    let mut reader = PacketReader::new(payload);
    let code = reader.read_u32()?;
    let message = if reader.is_empty() {
        String::new()
    } else {
        reader.read_string_lossy()?
    };
    Ok(SftpStatus { code, message })
}

fn status_to_error(status: SftpStatus, operation: String) -> HxError {
    let message = if status.message.is_empty() {
        format!("{operation} failed with SFTP status {}", status.code)
    } else {
        format!(
            "{operation} failed with SFTP status {}: {}",
            status.code, status.message
        )
    };
    HxError::Remote(message)
}

fn unexpected_response(operation: &str, typ: u8) -> HxError {
    HxError::Remote(format!(
        "{operation} returned unexpected SFTP packet type {typ}"
    ))
}

fn remote_io(err: io::Error) -> HxError {
    HxError::Remote(err.to_string())
}
