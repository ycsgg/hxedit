use std::env;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use suppaftp::types::{FileType, FtpError};
use suppaftp::{FtpStream, Mode};

use crate::error::{HxError, HxResult};

use super::{RemoteFingerprint, RemoteSave, RemoteStat, RemoteTarget};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) struct FtpBackend {
    target: RemoteTarget,
    readonly: bool,
    client: FtpClient,
}

struct FtpSave<D: Read + Write + 'static> {
    target: RemoteTarget,
    expected: Option<RemoteFingerprint>,
    client: FtpClient,
    data: Option<D>,
    temp_path: String,
    finished: bool,
}

struct FtpClient {
    stream: Box<FtpStream>,
}

impl fmt::Debug for FtpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FtpBackend")
            .field("target", &self.target.label())
            .field("readonly", &self.readonly)
            .finish_non_exhaustive()
    }
}

impl<D: Read + Write + 'static> fmt::Debug for FtpSave<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FtpSave")
            .field("target", &self.target.label())
            .field("temp_path", &self.temp_path)
            .finish_non_exhaustive()
    }
}

impl FtpBackend {
    pub(crate) fn open(target: RemoteTarget, readonly: bool) -> HxResult<Self> {
        validate_ftp_arg(target.path())?;
        let client = FtpClient::connect(&target)?;
        Ok(Self {
            target,
            readonly,
            client,
        })
    }

    pub(crate) fn read_at(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        self.client.read_at(self.target.path(), offset, len)
    }

    pub(crate) fn begin_save(
        &self,
        expected: Option<RemoteFingerprint>,
    ) -> HxResult<Box<dyn RemoteSave>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let mut client = FtpClient::connect(&self.target)?;
        let current = stat_with(&mut client, &self.target, self.readonly)?;
        if current.fingerprint != expected {
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        let temp_path = temp_path_for(self.target.path());
        let data = client.begin_store(&temp_path)?;
        Ok(Box::new(FtpSave {
            target: self.target.clone(),
            expected,
            client,
            data: Some(data),
            temp_path,
            finished: false,
        }))
    }

    pub(crate) fn reload(&mut self) -> HxResult<RemoteStat> {
        let stat = stat_with(&mut self.client, &self.target, self.readonly)?;
        Ok(stat)
    }
}

impl<D: Read + Write + 'static> Write for FtpSave<D> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let data = self
            .data
            .as_mut()
            .ok_or_else(|| io::Error::other("ftp data stream is already closed"))?;
        data.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(data) = self.data.as_mut() {
            data.flush()?;
        }
        Ok(())
    }
}

impl<D: Read + Write + 'static> RemoteSave for FtpSave<D> {
    fn finish(mut self: Box<Self>) -> HxResult<RemoteStat> {
        if let Some(mut data) = self.data.take() {
            if let Err(err) = data.flush() {
                let _ = self.client.abort_transfer(data);
                let _ = self.client.remove(&self.temp_path);
                return Err(remote_io(err));
            }
            if let Err(err) = self.client.finish_store(data) {
                let _ = self.client.remove(&self.temp_path);
                return Err(err);
            }
        }

        let current = stat_with(&mut self.client, &self.target, false)?;
        if current.fingerprint != self.expected {
            let _ = self.client.remove(&self.temp_path);
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        self.client
            .rename(&self.temp_path, self.target.path())
            .inspect_err(|_err| {
                let _ = self.client.remove(&self.temp_path);
            })?;
        self.finished = true;
        stat_with(&mut self.client, &self.target, false)
    }
}

impl<D: Read + Write + 'static> Drop for FtpSave<D> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Some(data) = self.data.take() {
            let _ = self.client.abort_transfer(data);
        }
        let _ = self.client.remove(&self.temp_path);
    }
}

impl FtpClient {
    fn connect(target: &RemoteTarget) -> HxResult<Self> {
        let port = target.port().unwrap_or(21);
        let stream = connect_tcp(target.host(), port)?;
        let stream = FtpStream::connect_with_stream(stream).map_err(remote_ftp)?;
        let mut stream = configure_passive_policy(stream)?;

        let (username, password) = ftp_credentials(target)?;
        stream.login(&username, &password).map_err(remote_ftp)?;
        stream.transfer_type(FileType::Binary).map_err(remote_ftp)?;
        Ok(Self {
            stream: Box::new(stream),
        })
    }

    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        validate_ftp_arg(path)?;
        if offset > 0 {
            let offset = usize::try_from(offset).map_err(|_| {
                HxError::Remote("FTP REST offset exceeds platform usize".to_owned())
            })?;
            self.stream.resume_transfer(offset).map_err(remote_ftp)?;
        }

        let mut data = self.stream.retr_as_stream(path).map_err(remote_ftp)?;
        let mut out = Vec::with_capacity(len);
        let limit = len
            .checked_add(1)
            .ok_or_else(|| HxError::Remote("FTP read length exceeds platform usize".to_owned()))?;
        let read_result = {
            let mut limited = Read::by_ref(&mut data).take(limit as u64);
            limited.read_to_end(&mut out)
        };
        if let Err(err) = read_result {
            return Err(remote_io(err));
        }
        if out.len() > len {
            out.truncate(len);
            self.abort_transfer(data)?;
        } else {
            self.stream.finalize_retr_stream(data).map_err(remote_ftp)?;
        }
        Ok(out)
    }

    fn begin_store(&mut self, path: &str) -> HxResult<impl Read + Write + 'static> {
        validate_ftp_arg(path)?;
        self.stream.put_with_stream(path).map_err(remote_ftp)
    }

    fn finish_store(&mut self, data: impl Write) -> HxResult<()> {
        self.stream.finalize_put_stream(data).map_err(remote_ftp)
    }

    fn abort_transfer(&mut self, data: impl Read + 'static) -> HxResult<()> {
        self.stream.abort(data).map_err(remote_ftp)
    }

    fn stat_path(&mut self, path: &str, readonly: bool) -> HxResult<RemoteStat> {
        validate_ftp_arg(path)?;
        let len = self.stream.size(path).map_err(remote_ftp)?;
        let len = u64::try_from(len)
            .map_err(|_| HxError::Remote("FTP SIZE response exceeds u64".to_owned()))?;

        let mdtm = self
            .stream
            .mdtm(path)
            .ok()
            .map(|value| format!("mdtm:{value}"));

        Ok(RemoteStat {
            len,
            fingerprint: Some(RemoteFingerprint {
                len,
                mtime: None,
                file_id: mdtm,
            }),
            readonly,
            permissions: None,
        })
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> HxResult<()> {
        validate_ftp_arg(old_path)?;
        validate_ftp_arg(new_path)?;
        self.stream.rename(old_path, new_path).map_err(remote_ftp)
    }

    fn remove(&mut self, path: &str) -> HxResult<()> {
        validate_ftp_arg(path)?;
        self.stream.rm(path).map_err(remote_ftp)
    }
}

impl Drop for FtpClient {
    fn drop(&mut self) {
        let _ = self.stream.quit();
    }
}

fn configure_passive_policy(stream: FtpStream) -> HxResult<FtpStream> {
    let peer = stream.get_ref().peer_addr().map_err(remote_io)?;
    let peer_ip = peer.ip();
    // Ignore PASV response hosts: a malicious server can otherwise make saves
    // connect and upload data to an unrelated internal or localhost address.
    let mut stream =
        stream.passive_stream_builder(move |addr| connect_passive_socket(peer_ip, addr.port()));
    if peer_ip.is_ipv6() {
        stream.set_mode(Mode::ExtendedPassive);
    } else {
        stream.set_mode(Mode::Passive);
    }
    Ok(stream)
}

fn stat_with(
    client: &mut FtpClient,
    target: &RemoteTarget,
    readonly: bool,
) -> HxResult<RemoteStat> {
    client.stat_path(target.path(), readonly)
}

fn ftp_credentials(target: &RemoteTarget) -> HxResult<(String, String)> {
    let username = target
        .username()
        .map(str::to_owned)
        .or_else(|| {
            env::var("HXEDIT_FTP_USER")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "anonymous".to_owned());
    validate_ftp_arg(&username)?;

    let password = match env::var("HXEDIT_FTP_PASSWORD") {
        Ok(value) => value,
        Err(_) if username.eq_ignore_ascii_case("anonymous") => "hxedit@example.invalid".to_owned(),
        Err(_) => {
            return Err(HxError::Remote(
                "FTP password missing; set HXEDIT_FTP_PASSWORD or use anonymous ftp://host/path"
                    .to_owned(),
            ));
        }
    };
    validate_ftp_arg(&password)?;
    Ok((username, password))
}

fn connect_tcp(host: &str, port: u16) -> HxResult<TcpStream> {
    let addrs = (host, port).to_socket_addrs().map_err(remote_io)?;
    let mut last_error = None;
    for addr in addrs {
        match connect_socket(addr) {
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

fn connect_socket(addr: SocketAddr) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(CONTROL_TIMEOUT)).ok();
    stream.set_write_timeout(Some(CONTROL_TIMEOUT)).ok();
    Ok(stream)
}

fn connect_passive_socket(peer_ip: IpAddr, port: u16) -> Result<TcpStream, FtpError> {
    connect_socket(SocketAddr::new(peer_ip, port)).map_err(FtpError::ConnectionError)
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

fn validate_ftp_arg(value: &str) -> HxResult<()> {
    if value.contains('\r') || value.contains('\n') {
        return Err(HxError::Remote(
            "FTP arguments must not contain CR or LF".to_owned(),
        ));
    }
    Ok(())
}

fn remote_ftp(err: FtpError) -> HxError {
    HxError::Remote(format!("FTP error: {err}"))
}

fn remote_io(err: impl std::fmt::Display) -> HxError {
    HxError::Remote(err.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn pasv_data_connection_uses_control_peer_ip() {
        let data = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        data.set_nonblocking(true).unwrap();
        let data_port = data.local_addr().unwrap().port();
        let data_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match data.accept() {
                    Ok((mut stream, _addr)) => {
                        stream.write_all(b"abcdef").unwrap();
                        return;
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "client did not connect to the expected PASV peer"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("data accept failed: {err}"),
                }
            }
        });

        let control = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let control_port = control.local_addr().unwrap().port();
        let control_thread = thread::spawn(move || {
            let (stream, _addr) = control.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            serve_pasv_read(stream, data_port);
        });

        let target = RemoteTarget::parse(&format!("ftp://127.0.0.1:{control_port}/file")).unwrap();
        let mut client = FtpClient::connect(&target).unwrap();
        let bytes = client.read_at(target.path(), 0, 6).unwrap();
        assert_eq!(bytes, b"abcdef");
        drop(client);

        data_thread.join().unwrap();
        control_thread.join().unwrap();
    }

    fn serve_pasv_read(mut stream: TcpStream, data_port: u16) {
        stream.write_all(b"220 fake ftp\r\n").unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let high = data_port / 256;
        let low = data_port % 256;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                return;
            }
            let command = line.trim_end_matches(['\r', '\n']);
            if command.starts_with("USER ") {
                stream.write_all(b"331 password needed\r\n").unwrap();
            } else if command.starts_with("PASS ") {
                stream.write_all(b"230 logged in\r\n").unwrap();
            } else if command == "TYPE I" {
                stream.write_all(b"200 binary\r\n").unwrap();
            } else if command == "PASV" {
                writeln!(
                    stream,
                    "227 Entering Passive Mode (127,0,0,2,{high},{low})\r"
                )
                .unwrap();
            } else if command.starts_with("RETR ") {
                stream.write_all(b"150 opening data\r\n").unwrap();
                thread::sleep(Duration::from_millis(50));
                stream.write_all(b"226 done\r\n").unwrap();
            } else if command == "QUIT" {
                stream.write_all(b"221 bye\r\n").ok();
                return;
            } else {
                stream.write_all(b"502 unsupported\r\n").unwrap();
            }
        }
    }
}
