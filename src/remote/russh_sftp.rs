use std::collections::HashSet;
use std::env;
use std::fmt;
use std::future::Future;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use directories::BaseDirs;
use russh::client::{self, AuthResult, Handler};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKey};
use russh::Disconnect;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::RawSftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags, Packet, Status, StatusCode};
use tokio::runtime::{Builder, Runtime};

use crate::error::{HxError, HxResult};

use super::{RemoteFingerprint, RemoteSave, RemoteStat, RemoteTarget};

const READ_CHUNK: usize = 256 * 1024;
const WRITE_CHUNK: usize = 32 * 1024;
const SFTP_TIMEOUT_SECS: u64 = 30;
const EXT_POSIX_RENAME: &str = "posix-rename@openssh.com";
const EXT_FSYNC: &str = "fsync@openssh.com";

pub(crate) struct RusshSftpBackend {
    target: RemoteTarget,
    readonly: bool,
    client: SftpClient,
    handle: String,
}

struct RusshSftpSave {
    target: RemoteTarget,
    expected: Option<RemoteFingerprint>,
    permissions: Option<u32>,
    client: SftpClient,
    handle: Option<String>,
    offset: u64,
    temp_path: String,
    finished: bool,
}

struct SftpClient {
    runtime: Runtime,
    ssh: client::Handle<ClientHandler>,
    sftp: RawSftpSession,
    extensions: HashSet<String>,
    read_chunk: usize,
    write_chunk: usize,
}

struct ClientHandler {
    host: String,
    port: u16,
    insecure: bool,
}

impl fmt::Debug for RusshSftpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RusshSftpBackend")
            .field("target", &self.target.label())
            .field("readonly", &self.readonly)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for RusshSftpSave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RusshSftpSave")
            .field("target", &self.target.label())
            .field("temp_path", &self.temp_path)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for SftpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SftpClient").finish_non_exhaustive()
    }
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let host = self.host.clone();
        let port = self.port;
        let insecure = self.insecure;
        let server_public_key = server_public_key.clone();
        async move {
            if insecure {
                return Ok(true);
            }
            match russh::keys::check_known_hosts(&host, port, &server_public_key) {
                Ok(known) => Ok(known),
                Err(russh::keys::Error::KeyChanged { line }) => {
                    Err(russh::Error::KeyChanged { line })
                }
                Err(_) => Ok(false),
            }
        }
    }
}

impl RusshSftpBackend {
    pub(crate) fn open(target: RemoteTarget, readonly: bool) -> HxResult<Self> {
        let client = SftpClient::connect(&target)?;
        let handle = client.open_path(target.path(), OpenFlags::READ)?;
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

        let client = SftpClient::connect(&self.target)?;
        let current = stat_with(&client, &self.target, self.readonly)?;
        if current.fingerprint != expected {
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        let temp_path = temp_path_for(self.target.path());
        let handle = client.open_path(
            &temp_path,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::EXCLUDE,
        )?;
        Ok(Box::new(RusshSftpSave {
            target: self.target.clone(),
            expected,
            permissions: current.permissions,
            client,
            handle: Some(handle),
            offset: 0,
            temp_path,
            finished: false,
        }))
    }

    pub(crate) fn reload(&mut self) -> HxResult<RemoteStat> {
        let stat = stat_with(&self.client, &self.target, self.readonly)?;
        let _ = self.client.close_handle(&self.handle);
        self.handle = self.client.open_path(self.target.path(), OpenFlags::READ)?;
        Ok(stat)
    }
}

impl Drop for RusshSftpBackend {
    fn drop(&mut self) {
        let _ = self.client.close_handle(&self.handle);
    }
}

impl Write for RusshSftpSave {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let handle = self
            .handle
            .as_deref()
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

impl RemoteSave for RusshSftpSave {
    fn finish(mut self: Box<Self>) -> HxResult<RemoteStat> {
        let current = stat_with(&self.client, &self.target, false)?;
        if current.fingerprint != self.expected {
            self.cleanup_temp();
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        if let Some(permissions) = self.permissions {
            let _ = self.client.set_permissions(&self.temp_path, permissions);
        }

        if let Some(handle) = self.handle.as_deref() {
            self.client.fsync_handle_if_supported(handle)?;
        }
        if let Some(handle) = self.handle.take() {
            self.client.close_handle(&handle)?;
        }

        if let Err(err) = self
            .client
            .posix_rename(&self.temp_path, self.target.path())
        {
            self.cleanup_temp();
            return Err(err);
        }

        self.finished = true;
        stat_with(&self.client, &self.target, false)
    }
}

impl Drop for RusshSftpSave {
    fn drop(&mut self) {
        if !self.finished {
            self.cleanup_temp();
        }
    }
}

impl RusshSftpSave {
    fn cleanup_temp(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = self.client.close_handle(&handle);
        }
        let _ = self.client.remove_path(&self.temp_path);
    }
}

impl SftpClient {
    fn connect(target: &RemoteTarget) -> HxResult<Self> {
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|err| HxError::Remote(format!("failed to start SSH runtime: {err}")))?;
        let (ssh, sftp, extensions, read_chunk, write_chunk) =
            runtime.block_on(connect_sftp(target))?;
        Ok(Self {
            runtime,
            ssh,
            sftp,
            extensions,
            read_chunk,
            write_chunk,
        })
    }

    fn open_path(&self, path: &str, flags: OpenFlags) -> HxResult<String> {
        self.runtime
            .block_on(async {
                self.sftp
                    .open(path.to_owned(), flags, FileAttributes::empty())
                    .await
            })
            .map(|handle| handle.handle)
            .map_err(|err| sftp_error(format!("open {path}"), err))
    }

    fn read_at(&self, handle: &str, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        self.runtime.block_on(async {
            let mut out = Vec::with_capacity(len);
            while out.len() < len {
                let remaining = len - out.len();
                let count = self.read_chunk.min(remaining);
                let request_len = u32::try_from(count).map_err(|_| {
                    HxError::Remote(format!("SFTP read request too large: {count}"))
                })?;
                match self
                    .sftp
                    .read(handle.to_owned(), offset + out.len() as u64, request_len)
                    .await
                {
                    Ok(data) if data.data.is_empty() => break,
                    Ok(data) => {
                        let take = data.data.len().min(len - out.len());
                        out.extend_from_slice(&data.data[..take]);
                    }
                    Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => {
                        break;
                    }
                    Err(err) => return Err(sftp_error("read", err)),
                }
            }
            Ok(out)
        })
    }

    fn write_all_at(&self, handle: &str, mut offset: u64, mut buf: &[u8]) -> HxResult<()> {
        self.runtime.block_on(async {
            while !buf.is_empty() {
                let count = self.write_chunk.min(buf.len());
                let chunk = buf[..count].to_vec();
                self.sftp
                    .write(handle.to_owned(), offset, chunk)
                    .await
                    .map_err(|err| sftp_error("write", err))?;
                offset += count as u64;
                buf = &buf[count..];
            }
            Ok(())
        })
    }

    fn close_handle(&self, handle: &str) -> HxResult<()> {
        self.runtime
            .block_on(async { self.sftp.close(handle.to_owned()).await })
            .map(|_| ())
            .map_err(|err| sftp_error("close", err))
    }

    fn fsync_handle_if_supported(&self, handle: &str) -> HxResult<()> {
        if !self.extensions.contains(EXT_FSYNC) {
            return Ok(());
        }
        self.runtime
            .block_on(async { self.sftp.fsync(handle.to_owned()).await })
            .map(|_| ())
            .map_err(|err| sftp_error("fsync", err))
    }

    fn stat_path(&self, path: &str) -> HxResult<FileAttributes> {
        self.runtime
            .block_on(async { self.sftp.stat(path.to_owned()).await })
            .map(|attrs| attrs.attrs)
            .map_err(|err| sftp_error(format!("stat {path}"), err))
    }

    fn set_permissions(&self, path: &str, permissions: u32) -> HxResult<()> {
        let mut attrs = FileAttributes::empty();
        attrs.permissions = Some(permissions);
        self.runtime
            .block_on(async { self.sftp.setstat(path.to_owned(), attrs).await })
            .map(|_| ())
            .map_err(|err| sftp_error("setstat", err))
    }

    fn remove_path(&self, path: &str) -> HxResult<()> {
        self.runtime
            .block_on(async { self.sftp.remove(path.to_owned()).await })
            .map(|_| ())
            .map_err(|err| sftp_error(format!("remove {path}"), err))
    }

    fn posix_rename(&self, old_path: &str, new_path: &str) -> HxResult<()> {
        if !self.extensions.contains(EXT_POSIX_RENAME) {
            return Err(HxError::Remote(
                "SFTP server does not advertise posix-rename@openssh.com; refusing non-atomic overwrite save"
                    .to_owned(),
            ));
        }

        let mut payload = Vec::new();
        put_string(&mut payload, old_path.as_bytes())?;
        put_string(&mut payload, new_path.as_bytes())?;
        let packet = self
            .runtime
            .block_on(async { self.sftp.extended(EXT_POSIX_RENAME, payload).await })
            .map_err(|err| sftp_error("posix-rename", err))?;
        status_packet_to_result("posix-rename", packet)
    }
}

impl Drop for SftpClient {
    fn drop(&mut self) {
        let _ = self.sftp.close_session();
        let _ = self.runtime.block_on(async {
            self.ssh
                .disconnect(Disconnect::ByApplication, "hxedit closing SFTP session", "")
                .await
        });
    }
}

async fn connect_sftp(
    target: &RemoteTarget,
) -> HxResult<(
    client::Handle<ClientHandler>,
    RawSftpSession,
    HashSet<String>,
    usize,
    usize,
)> {
    let host = target.host().to_owned();
    let port = target.port().unwrap_or(22);
    let username = username_for(target)?;
    let handler = ClientHandler {
        host: host.clone(),
        port,
        insecure: env::var_os("HXEDIT_SFTP_INSECURE").is_some(),
    };
    let mut config = client::Config {
        nodelay: true,
        ..client::Config::default()
    };
    config.keepalive_interval = Some(std::time::Duration::from_secs(30));
    let mut ssh = client::connect(Arc::new(config), (host.as_str(), port), handler)
        .await
        .map_err(|err| HxError::Remote(format!("failed to connect SSH: {err}")))?;

    authenticate(&mut ssh, &username).await?;

    let channel = ssh
        .channel_open_session()
        .await
        .map_err(|err| HxError::Remote(format!("failed to open SSH session channel: {err}")))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|err| HxError::Remote(format!("failed to start SFTP subsystem: {err}")))?;

    let mut sftp = RawSftpSession::new(channel.into_stream());
    sftp.set_timeout(SFTP_TIMEOUT_SECS);
    let version = sftp.init().await.map_err(|err| sftp_error("init", err))?;
    if version.version != 3 {
        return Err(HxError::Remote(format!(
            "unsupported SFTP protocol version {}; expected 3",
            version.version
        )));
    }

    let mut read_chunk = READ_CHUNK;
    let mut write_chunk = WRITE_CHUNK;
    if version
        .extensions
        .contains_key(russh_sftp::extensions::LIMITS)
    {
        if let Ok(limits) = sftp.limits().await {
            if limits.max_read_len > 0 {
                read_chunk = read_chunk.min(limits.max_read_len.min(usize::MAX as u64) as usize);
            }
            if limits.max_write_len > 0 {
                write_chunk = write_chunk.min(limits.max_write_len.min(usize::MAX as u64) as usize);
            }
            sftp.set_limits(limits.into());
        }
    }
    read_chunk = read_chunk.max(1);
    write_chunk = write_chunk.max(1);

    Ok((
        ssh,
        sftp,
        version.extensions.keys().cloned().collect(),
        read_chunk,
        write_chunk,
    ))
}

async fn authenticate(ssh: &mut client::Handle<ClientHandler>, username: &str) -> HxResult<()> {
    let mut failures = Vec::new();

    match try_agent_auth(ssh, username).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(err) => failures.push(err),
    }

    match try_default_key_auth(ssh, username).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(err) => failures.push(err),
    }

    if let Some(password) = env::var("HXEDIT_SFTP_PASSWORD").ok() {
        match ssh
            .authenticate_password(username.to_owned(), password)
            .await
        {
            Ok(result) if result.success() => return Ok(()),
            Ok(_) => failures.push("password rejected".to_owned()),
            Err(err) => failures.push(format!("password auth failed: {err}")),
        }
    }

    let detail = if failures.is_empty() {
        "no usable Unix ssh-agent identity, default private key, or HXEDIT_SFTP_PASSWORD".to_owned()
    } else {
        failures.join("; ")
    };
    Err(HxError::Remote(format!(
        "SSH authentication failed for {username}: {detail}"
    )))
}

#[cfg(unix)]
async fn try_agent_auth(
    ssh: &mut client::Handle<ClientHandler>,
    username: &str,
) -> Result<bool, String> {
    use russh::keys::agent::client::AgentClient;

    if env::var_os("SSH_AUTH_SOCK").is_none() {
        return Ok(false);
    }
    let mut agent = AgentClient::connect_env()
        .await
        .map_err(|err| format!("ssh-agent unavailable: {err}"))?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|err| format!("ssh-agent identities unavailable: {err}"))?;
    if identities.is_empty() {
        return Ok(false);
    }

    let mut rejected = 0usize;
    for identity in identities {
        let key = identity.public_key().into_owned();
        let hash_alg = rsa_hash_for(ssh, key.algorithm().is_rsa()).await;
        match ssh
            .authenticate_publickey_with(username.to_owned(), key, hash_alg, &mut agent)
            .await
        {
            Ok(AuthResult::Success) => return Ok(true),
            Ok(AuthResult::Failure { .. }) => rejected += 1,
            Err(err) => return Err(format!("ssh-agent auth failed: {err}")),
        }
    }

    if rejected > 0 {
        Err(format!("{rejected} ssh-agent identities rejected"))
    } else {
        Err("ssh-agent did not provide usable identities".to_owned())
    }
}

#[cfg(not(unix))]
async fn try_agent_auth(
    _ssh: &mut client::Handle<ClientHandler>,
    _username: &str,
) -> Result<bool, String> {
    Ok(false)
}

async fn try_default_key_auth(
    ssh: &mut client::Handle<ClientHandler>,
    username: &str,
) -> Result<bool, String> {
    let Some(home) = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) else {
        return Ok(false);
    };
    let ssh_dir = home.join(".ssh");
    let mut saw_key = false;
    let mut failures = Vec::new();

    for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
        let path = ssh_dir.join(name);
        if !path.is_file() {
            continue;
        }
        saw_key = true;
        let key = match load_secret_key(&path, None) {
            Ok(key) => Arc::new(key),
            Err(err) => {
                failures.push(format!("{}: {err}", path.display()));
                continue;
            }
        };
        let hash_alg = rsa_hash_for(ssh, key.algorithm().is_rsa()).await;
        let key = PrivateKeyWithHashAlg::new(key, hash_alg);
        match ssh.authenticate_publickey(username.to_owned(), key).await {
            Ok(AuthResult::Success) => return Ok(true),
            Ok(AuthResult::Failure { .. }) => {
                failures.push(format!("{} rejected", path.display()));
            }
            Err(err) => failures.push(format!("{} auth failed: {err}", path.display())),
        }
    }

    if saw_key && !failures.is_empty() {
        Err(failures.join("; "))
    } else {
        Ok(false)
    }
}

async fn rsa_hash_for(
    ssh: &client::Handle<ClientHandler>,
    is_rsa: bool,
) -> Option<russh::keys::HashAlg> {
    if !is_rsa {
        return None;
    }
    ssh.best_supported_rsa_hash().await.ok().flatten().flatten()
}

fn username_for(target: &RemoteTarget) -> HxResult<String> {
    if let Some(username) = target.username() {
        return Ok(username.to_owned());
    }
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .map_err(|_| {
            HxError::Remote(
                "remote username missing; use sftp://user@host/path or set USER".to_owned(),
            )
        })
}

fn stat_with(client: &SftpClient, target: &RemoteTarget, readonly: bool) -> HxResult<RemoteStat> {
    let stat = client.stat_path(target.path())?;
    let len = stat
        .size
        .ok_or_else(|| HxError::Remote(format!("remote target has no size: {}", target.label())))?;
    Ok(RemoteStat {
        len,
        fingerprint: Some(RemoteFingerprint {
            len,
            mtime: stat.mtime.map(u64::from),
            file_id: stat.permissions.map(|perm| format!("perm:{perm:o}")),
        }),
        readonly,
        permissions: stat.permissions,
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

fn status_packet_to_result(operation: &str, packet: Packet) -> HxResult<()> {
    match packet {
        Packet::Status(status) if status.status_code == StatusCode::Ok => Ok(()),
        Packet::Status(status) => Err(status_error(operation, status)),
        _ => Err(HxError::Remote(format!(
            "{operation} returned unexpected SFTP packet"
        ))),
    }
}

fn sftp_error(operation: impl AsRef<str>, error: SftpError) -> HxError {
    match error {
        SftpError::Status(status) => status_error(operation.as_ref(), status),
        other => HxError::Remote(format!("{} failed: {other}", operation.as_ref())),
    }
}

fn status_error(operation: &str, status: Status) -> HxError {
    if status.error_message.is_empty() {
        HxError::Remote(format!(
            "{operation} failed with SFTP status {}",
            status.status_code
        ))
    } else {
        HxError::Remote(format!(
            "{operation} failed with SFTP status {}: {}",
            status.status_code, status.error_message
        ))
    }
}

fn put_string(out: &mut Vec<u8>, value: &[u8]) -> HxResult<()> {
    let len = u32::try_from(value.len())
        .map_err(|_| HxError::Remote("SFTP extension string too large".to_owned()))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}
