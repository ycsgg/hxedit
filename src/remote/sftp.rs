use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ssh2::{CheckResult, File, FileStat, KnownHostFileKind, RenameFlags, Session, Sftp};

use crate::error::{HxError, HxResult};

use super::{RemoteFingerprint, RemoteSave, RemoteStat, RemoteTarget};

pub(crate) struct SftpBackend {
    target: RemoteTarget,
    readonly: bool,
    _session: Session,
    sftp: Sftp,
    file: File,
}

struct SftpSave {
    target: RemoteTarget,
    expected: Option<RemoteFingerprint>,
    permissions: Option<u32>,
    _session: Session,
    sftp: Sftp,
    file: Option<File>,
    temp_path: String,
}

#[derive(Debug, Clone)]
struct SshConfig {
    host_name: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct ResolvedSshTarget {
    host: String,
    port: u16,
    username: String,
    identity_files: Vec<PathBuf>,
}

impl fmt::Debug for SftpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SftpBackend")
            .field("target", &self.target.label())
            .field("readonly", &self.readonly)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for SftpSave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SftpSave")
            .field("target", &self.target.label())
            .field("temp_path", &self.temp_path)
            .finish_non_exhaustive()
    }
}

impl SftpBackend {
    pub(crate) fn open(target: RemoteTarget, readonly: bool) -> HxResult<Self> {
        let (session, sftp) = connect(&target)?;
        let file = open_file(&sftp, &target)?;
        Ok(Self {
            target,
            readonly,
            _session: session,
            sftp,
            file,
        })
    }

    pub(crate) fn read_at(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(remote_error)?;
        let mut out = vec![0; len];
        let mut total = 0;
        while total < len {
            let read = self.file.read(&mut out[total..]).map_err(remote_error)?;
            if read == 0 {
                break;
            }
            total += read;
        }
        out.truncate(total);
        Ok(out)
    }

    pub(crate) fn begin_save(
        &self,
        expected: Option<RemoteFingerprint>,
    ) -> HxResult<Box<dyn RemoteSave>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let current = stat_with(&self.sftp, &self.target, self.readonly)?;
        if current.fingerprint != expected {
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        let (session, sftp) = connect(&self.target)?;
        let temp_path = temp_path_for(self.target.path());
        let file = sftp.create(Path::new(&temp_path)).map_err(remote_error)?;
        Ok(Box::new(SftpSave {
            target: self.target.clone(),
            expected,
            permissions: current.permissions,
            _session: session,
            sftp,
            file: Some(file),
            temp_path,
        }))
    }

    pub(crate) fn reload(&mut self) -> HxResult<RemoteStat> {
        let stat = stat_with(&self.sftp, &self.target, self.readonly)?;
        self.file = open_file(&self.sftp, &self.target)?;
        Ok(stat)
    }
}

impl Write for SftpSave {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("remote temp file is already closed"))?;
        file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }
}

impl RemoteSave for SftpSave {
    fn finish(mut self: Box<Self>) -> HxResult<RemoteStat> {
        if let Some(mut file) = self.file.take() {
            file.flush().map_err(remote_error)?;
        }

        let current = stat_with(&self.sftp, &self.target, false)?;
        if current.fingerprint != self.expected {
            let _ = self.sftp.unlink(Path::new(&self.temp_path));
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        if let Some(permissions) = self.permissions {
            let stat = FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: Some(permissions),
                atime: None,
                mtime: None,
            };
            let _ = self.sftp.setstat(Path::new(&self.temp_path), stat);
        }

        self.sftp
            .rename(
                Path::new(&self.temp_path),
                Path::new(self.target.path()),
                Some(RenameFlags::OVERWRITE),
            )
            .map_err(|err| {
                let _ = self.sftp.unlink(Path::new(&self.temp_path));
                remote_error(err)
            })?;

        stat_with(&self.sftp, &self.target, false)
    }
}

fn connect(target: &RemoteTarget) -> HxResult<(Session, Sftp)> {
    let resolved = resolve_ssh_target(target)?;
    let tcp = connect_tcp(&resolved.host, resolved.port)?;
    let mut session = Session::new().map_err(remote_error)?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(remote_error)?;
    verify_host_key(&session, target, &resolved)?;

    authenticate(&session, &resolved)?;
    let sftp = session.sftp().map_err(remote_error)?;
    Ok((session, sftp))
}

fn open_file(sftp: &Sftp, target: &RemoteTarget) -> HxResult<File> {
    sftp.open(Path::new(target.path())).map_err(remote_error)
}

fn connect_tcp(host: &str, port: u16) -> HxResult<TcpStream> {
    let addrs = (host, port).to_socket_addrs().map_err(remote_error)?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(8)) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err),
        }
    }
    Err(HxError::Remote(format!(
        "failed to connect to {host}:{port}: {}",
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "no resolved addresses".to_owned())
    )))
}

fn authenticate(session: &Session, resolved: &ResolvedSshTarget) -> HxResult<()> {
    let mut failures = Vec::new();
    if let Err(err) = session.userauth_agent(&resolved.username) {
        failures.push(format!("ssh-agent: {err}"));
    }
    if session.authenticated() {
        return Ok(());
    }

    for identity in &resolved.identity_files {
        if !identity.exists() {
            continue;
        }
        let public_key = public_key_path(identity);
        let public_key = public_key.as_deref().filter(|path| path.exists());
        match session.userauth_pubkey_file(&resolved.username, public_key, identity, None) {
            Ok(()) if session.authenticated() => return Ok(()),
            Ok(()) => failures.push(format!("{}: not accepted", identity.display())),
            Err(err) => failures.push(format!("{}: {err}", identity.display())),
        }
    }

    Err(HxError::Remote(format!(
        "authentication failed for {}@{}; tried {}",
        resolved.username,
        resolved.host,
        if failures.is_empty() {
            "no identities".to_owned()
        } else {
            failures.join("; ")
        }
    )))
}

fn verify_host_key(
    session: &Session,
    target: &RemoteTarget,
    resolved: &ResolvedSshTarget,
) -> HxResult<()> {
    if env::var_os("HXEDIT_SFTP_INSECURE").is_some() {
        return Ok(());
    }
    let (key, _) = session.host_key().ok_or_else(|| {
        HxError::Remote(format!(
            "server {} did not provide a host key",
            target.host()
        ))
    })?;

    let mut known_hosts = session.known_hosts().map_err(remote_error)?;
    let path = known_hosts_path().ok_or_else(|| {
        HxError::Remote(
            "cannot locate ~/.ssh/known_hosts; set HXEDIT_SFTP_INSECURE=1 to skip host-key checking"
                .to_owned(),
        )
    })?;
    known_hosts
        .read_file(&path, KnownHostFileKind::OpenSSH)
        .map_err(remote_error)?;
    let check = match known_hosts.check_port(&resolved.host, resolved.port, key) {
        CheckResult::NotFound if resolved.host != target.host() => {
            known_hosts.check_port(target.host(), resolved.port, key)
        }
        check => check,
    };
    match check {
        CheckResult::Match => Ok(()),
        CheckResult::Mismatch => Err(HxError::Remote(format!(
            "host key mismatch for {}",
            resolved.host
        ))),
        CheckResult::NotFound => Err(HxError::Remote(format!(
            "host key for {} not found in {}; connect with ssh first or set HXEDIT_SFTP_INSECURE=1",
            resolved.host,
            path.display()
        ))),
        CheckResult::Failure => Err(HxError::Remote(format!(
            "failed to check host key for {}",
            resolved.host
        ))),
    }
}

fn resolve_ssh_target(target: &RemoteTarget) -> HxResult<ResolvedSshTarget> {
    let config = load_ssh_config(target.host());
    let host = config.host_name.unwrap_or_else(|| target.host().to_owned());
    let port = target.port().or(config.port).unwrap_or(22);
    let username = target
        .username()
        .map(str::to_owned)
        .or(config.user)
        .or_else(default_username)
        .ok_or_else(|| {
            HxError::Remote(
                "remote username missing; use sftp://user@host/path, SSH config User, or USER"
                    .to_owned(),
            )
        })?;
    let mut identity_files = config.identity_files;
    identity_files.extend(default_identity_files());
    identity_files = identity_files
        .into_iter()
        .map(|path| expand_identity_path(path, &host, port, &username))
        .collect();
    identity_files.dedup();
    Ok(ResolvedSshTarget {
        host,
        port,
        username,
        identity_files,
    })
}

fn load_ssh_config(alias: &str) -> SshConfig {
    let mut resolved = SshConfig {
        host_name: None,
        user: None,
        port: None,
        identity_files: Vec::new(),
    };
    let Some(path) = home_dir().map(|home| home.join(".ssh").join("config")) else {
        return resolved;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return resolved;
    };
    let mut active = false;
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        if key.eq_ignore_ascii_case("host") {
            active = parts.any(|pattern| ssh_host_pattern_matches(pattern, alias));
            continue;
        }
        if !active {
            continue;
        }
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "hostname" if resolved.host_name.is_none() => resolved.host_name = Some(value),
            "user" if resolved.user.is_none() => resolved.user = Some(value),
            "port" if resolved.port.is_none() => {
                if let Ok(port) = value.parse() {
                    resolved.port = Some(port);
                }
            }
            "identityfile" => resolved.identity_files.push(PathBuf::from(value)),
            _ => {}
        }
    }
    resolved
}

fn ssh_host_pattern_matches(pattern: &str, alias: &str) -> bool {
    if let Some(negated) = pattern.strip_prefix('!') {
        return !ssh_host_pattern_matches(negated, alias);
    }
    wildcard_match(pattern, alias)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut star_text) = (None, 0usize);
    let p_bytes = pattern.as_bytes();
    let t_bytes = text.as_bytes();
    while t < t_bytes.len() {
        if p < p_bytes.len() && (p_bytes[p] == b'?' || p_bytes[p] == t_bytes[t]) {
            p += 1;
            t += 1;
        } else if p < p_bytes.len() && p_bytes[p] == b'*' {
            star = Some(p);
            p += 1;
            star_text = t;
        } else if let Some(star_pos) = star {
            p = star_pos + 1;
            star_text += 1;
            t = star_text;
        } else {
            return false;
        }
    }
    while p < p_bytes.len() && p_bytes[p] == b'*' {
        p += 1;
    }
    p == p_bytes.len()
}

fn stat_with(sftp: &Sftp, target: &RemoteTarget, readonly: bool) -> HxResult<RemoteStat> {
    let stat = sftp.stat(Path::new(target.path())).map_err(remote_error)?;
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

fn known_hosts_path() -> Option<PathBuf> {
    let home = home_dir()?;
    let path = home.join(".ssh").join("known_hosts");
    fs::metadata(&path).ok()?;
    Some(path)
}

fn default_identity_files() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    ["id_ed25519", "id_ecdsa", "id_rsa"]
        .into_iter()
        .map(|name| home.join(".ssh").join(name))
        .collect()
}

fn expand_identity_path(path: PathBuf, host: &str, port: u16, username: &str) -> PathBuf {
    let mut text = path.to_string_lossy().into_owned();
    if let Some(home) = home_dir() {
        if text == "~" {
            text = home.to_string_lossy().into_owned();
        } else if let Some(rest) = text.strip_prefix("~/") {
            text = home.join(rest).to_string_lossy().into_owned();
        }
    }
    text = text
        .replace("%h", host)
        .replace("%p", &port.to_string())
        .replace("%r", username);
    PathBuf::from(text)
}

fn public_key_path(identity: &Path) -> Option<PathBuf> {
    let file_name = identity.file_name()?.to_str()?;
    let mut public = identity.to_path_buf();
    public.set_file_name(format!("{file_name}.pub"));
    Some(public)
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn default_username() -> Option<String> {
    env::var("USER")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| env::var("USERNAME").ok().filter(|value| !value.is_empty()))
}

fn remote_error(err: impl std::fmt::Display) -> HxError {
    HxError::Remote(err.to_string())
}
