use std::fmt;
use std::io::Write;

use crate::error::{HxError, HxResult};

#[cfg(test)]
mod fake;
#[cfg(feature = "remote-ftp")]
mod ftp;
#[cfg(feature = "remote-sftp")]
mod russh_sftp;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteTarget {
    scheme: String,
    username: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFingerprint {
    pub len: u64,
    pub mtime: Option<u64>,
    pub file_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteStat {
    pub len: u64,
    pub fingerprint: Option<RemoteFingerprint>,
    pub readonly: bool,
    pub permissions: Option<u32>,
}

pub trait RemoteSave: Write + fmt::Debug {
    fn finish(self: Box<Self>) -> HxResult<RemoteStat>;
}

#[derive(Debug)]
pub struct RemoteSource {
    target: RemoteTarget,
    stat: RemoteStat,
    #[allow(dead_code)]
    backend: RemoteBackend,
}

#[derive(Debug)]
enum RemoteBackend {
    #[cfg(test)]
    Fake(fake::FakeBackend),
    #[cfg(feature = "remote-sftp")]
    RusshSftp(Box<russh_sftp::RusshSftpBackend>),
    #[cfg(feature = "remote-ftp")]
    Ftp(ftp::FtpBackend),
    #[allow(dead_code)]
    Unavailable,
}

impl RemoteTarget {
    pub fn parse(input: &str) -> HxResult<Self> {
        let (scheme, rest) = input.split_once("://").ok_or_else(|| {
            HxError::Remote(format!(
                "invalid remote target {input}; expected scheme://host/path"
            ))
        })?;
        if scheme.is_empty() {
            return Err(HxError::Remote(
                "remote scheme must not be empty".to_owned(),
            ));
        }

        let (authority, path) = rest.split_once('/').ok_or_else(|| {
            HxError::Remote(format!(
                "invalid remote target {input}; expected scheme://host/path"
            ))
        })?;
        if authority.is_empty() {
            return Err(HxError::Remote("remote host must not be empty".to_owned()));
        }

        let (username, host_port) = match authority.rsplit_once('@') {
            Some((userinfo, host_port)) => {
                if userinfo.contains(':') {
                    return Err(HxError::Remote(
                        "passwords in remote URIs are not supported; use Unix ssh-agent, SSH keys, or HXEDIT_SFTP_PASSWORD"
                            .to_owned(),
                    ));
                }
                if userinfo.is_empty() {
                    (None, host_port)
                } else {
                    (Some(userinfo.to_owned()), host_port)
                }
            }
            None => (None, authority),
        };

        let (host, port) = parse_host_port(host_port)?;
        let path = format!("/{path}");
        if path == "/" {
            return Err(HxError::Remote("remote path must not be empty".to_owned()));
        }

        Ok(Self {
            scheme: scheme.to_ascii_lowercase(),
            username,
            host,
            port,
            path,
        })
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn label(&self) -> String {
        let user = self
            .username
            .as_ref()
            .map(|user| format!("{user}@"))
            .unwrap_or_default();
        let port = self.port.map(|port| format!(":{port}")).unwrap_or_default();
        format!(
            "{}://{}{}{}{}",
            self.scheme, user, self.host, port, self.path
        )
    }

    #[cfg(test)]
    fn key(&self) -> String {
        self.label()
    }
}

impl RemoteSource {
    pub fn open(target: RemoteTarget, readonly: bool) -> HxResult<Self> {
        match target.scheme() {
            #[cfg(test)]
            "fake" => {
                let mut backend = fake::FakeBackend::open(target.clone(), readonly)?;
                let stat = backend.reload()?;
                Ok(Self {
                    target,
                    stat,
                    backend: RemoteBackend::Fake(backend),
                })
            }
            "sftp" => open_sftp(target, readonly),
            "ssh" => open_sftp(target, readonly),
            "ftp" => open_ftp(target, readonly),
            other => Err(HxError::Remote(format!(
                "unsupported remote scheme {other}; known schemes: sftp, ssh, ftp"
            ))),
        }
    }

    pub fn label(&self) -> String {
        self.target.label()
    }

    pub fn len(&self) -> u64 {
        self.stat.len
    }

    pub fn is_empty(&self) -> bool {
        self.stat.len == 0
    }

    pub fn readonly(&self) -> bool {
        self.stat.readonly
    }

    pub fn read_at(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        #[cfg(not(any(test, feature = "remote-sftp", feature = "remote-ftp")))]
        {
            let _ = (offset, len);
            Err(HxError::Remote(
                "remote backend unavailable in this build".to_owned(),
            ))
        }

        #[cfg(any(test, feature = "remote-sftp", feature = "remote-ftp"))]
        match &mut self.backend {
            #[cfg(test)]
            RemoteBackend::Fake(backend) => backend.read_at(offset, len),
            #[cfg(feature = "remote-sftp")]
            RemoteBackend::RusshSftp(backend) => backend.read_at(offset, len),
            #[cfg(feature = "remote-ftp")]
            RemoteBackend::Ftp(backend) => backend.read_at(offset, len),
            RemoteBackend::Unavailable => Err(HxError::Remote(
                "remote backend unavailable in this build".to_owned(),
            )),
        }
    }

    pub fn begin_save(&self) -> HxResult<Box<dyn RemoteSave>> {
        if self.stat.readonly {
            return Err(HxError::ReadOnly);
        }
        #[cfg(not(any(test, feature = "remote-sftp", feature = "remote-ftp")))]
        {
            Err(HxError::Remote(
                "remote backend unavailable in this build".to_owned(),
            ))
        }

        #[cfg(any(test, feature = "remote-sftp", feature = "remote-ftp"))]
        match &self.backend {
            #[cfg(test)]
            RemoteBackend::Fake(backend) => backend.begin_save(self.stat.fingerprint.clone()),
            #[cfg(feature = "remote-sftp")]
            RemoteBackend::RusshSftp(backend) => backend.begin_save(self.stat.fingerprint.clone()),
            #[cfg(feature = "remote-ftp")]
            RemoteBackend::Ftp(backend) => backend.begin_save(self.stat.fingerprint.clone()),
            RemoteBackend::Unavailable => Err(HxError::Remote(
                "remote backend unavailable in this build".to_owned(),
            )),
        }
    }

    pub fn complete_save(&mut self, stat: RemoteStat) {
        self.stat = stat;
    }

    pub fn reload(&mut self) -> HxResult<RemoteStat> {
        #[cfg(not(any(test, feature = "remote-sftp", feature = "remote-ftp")))]
        {
            Err(HxError::Remote(
                "remote backend unavailable in this build".to_owned(),
            ))
        }

        #[cfg(any(test, feature = "remote-sftp", feature = "remote-ftp"))]
        {
            let stat: RemoteStat = match &mut self.backend {
                #[cfg(test)]
                RemoteBackend::Fake(backend) => backend.reload()?,
                #[cfg(feature = "remote-sftp")]
                RemoteBackend::RusshSftp(backend) => backend.reload()?,
                #[cfg(feature = "remote-ftp")]
                RemoteBackend::Ftp(backend) => backend.reload()?,
                RemoteBackend::Unavailable => {
                    return Err(HxError::Remote(
                        "remote backend unavailable in this build".to_owned(),
                    ));
                }
            };
            self.stat = stat.clone();
            Ok(stat)
        }
    }
}

fn parse_host_port(input: &str) -> HxResult<(String, Option<u16>)> {
    if let Some(rest) = input.strip_prefix('[') {
        let (host, after_host) = rest
            .split_once(']')
            .ok_or_else(|| HxError::Remote("invalid bracketed IPv6 host".to_owned()))?;
        if host.is_empty() {
            return Err(HxError::Remote("remote host must not be empty".to_owned()));
        }
        let port = if let Some(port) = after_host.strip_prefix(':') {
            Some(parse_port(port)?)
        } else if after_host.is_empty() {
            None
        } else {
            return Err(HxError::Remote("invalid host/port syntax".to_owned()));
        };
        return Ok((host.to_owned(), port));
    }

    match input.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            if host.is_empty() {
                return Err(HxError::Remote("remote host must not be empty".to_owned()));
            }
            Ok((host.to_owned(), Some(parse_port(port)?)))
        }
        _ => Ok((input.to_owned(), None)),
    }
}

fn parse_port(input: &str) -> HxResult<u16> {
    input
        .parse::<u16>()
        .map_err(|_| HxError::Remote(format!("invalid remote port {input}")))
}

#[cfg(feature = "remote-sftp")]
fn open_sftp(target: RemoteTarget, readonly: bool) -> HxResult<RemoteSource> {
    let mut backend = russh_sftp::RusshSftpBackend::open(target.clone(), readonly)?;
    let stat = backend.reload()?;
    Ok(RemoteSource {
        target,
        stat,
        backend: RemoteBackend::RusshSftp(Box::new(backend)),
    })
}

#[cfg(not(feature = "remote-sftp"))]
fn open_sftp(target: RemoteTarget, _readonly: bool) -> HxResult<RemoteSource> {
    let _ = target;
    Err(HxError::Remote(
        "sftp/ssh remote support requires building with --features remote-sftp".to_owned(),
    ))
}

#[cfg(feature = "remote-ftp")]
fn open_ftp(target: RemoteTarget, readonly: bool) -> HxResult<RemoteSource> {
    let mut backend = ftp::FtpBackend::open(target.clone(), readonly)?;
    let stat = backend.reload()?;
    Ok(RemoteSource {
        target,
        stat,
        backend: RemoteBackend::Ftp(backend),
    })
}

#[cfg(not(feature = "remote-ftp"))]
fn open_ftp(target: RemoteTarget, _readonly: bool) -> HxResult<RemoteSource> {
    let _ = target;
    Err(HxError::Remote(
        "ftp remote support requires building with --features remote-ftp".to_owned(),
    ))
}

#[cfg(test)]
pub(crate) use fake::{fake_bytes, install_fake, touch as touch_fake};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sftp_target() {
        let target = RemoteTarget::parse("sftp://alice@example.com:2222/var/tmp/blob.bin").unwrap();
        assert_eq!(target.scheme(), "sftp");
        assert_eq!(target.username(), Some("alice"));
        assert_eq!(target.host(), "example.com");
        assert_eq!(target.port(), Some(2222));
        assert_eq!(target.path(), "/var/tmp/blob.bin");
        assert_eq!(
            target.label(),
            "sftp://alice@example.com:2222/var/tmp/blob.bin"
        );
    }

    #[test]
    fn parses_ssh_alias_target() {
        let target = RemoteTarget::parse("ssh://example.com/var/tmp/blob.bin").unwrap();
        assert_eq!(target.scheme(), "ssh");
        assert_eq!(target.host(), "example.com");
        assert_eq!(target.path(), "/var/tmp/blob.bin");
    }

    #[test]
    fn rejects_password_in_uri() {
        let err = RemoteTarget::parse("sftp://alice:secret@example.com/file").unwrap_err();
        assert!(err.to_string().contains("passwords in remote URIs"));
    }

    #[test]
    fn fake_backend_detects_conflict() {
        let target = install_fake("fake://unit/conflict.bin", b"abcd", false);
        let source = RemoteSource::open(target.clone(), false).unwrap();
        fake::touch(&target, b"changed");

        let mut writer = source.begin_save().unwrap();
        writer.write_all(b"ABCD").unwrap();
        let err = writer.finish().unwrap_err();

        assert!(matches!(err, HxError::RemoteConflict { .. }));
    }
}
