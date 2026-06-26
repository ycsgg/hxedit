use std::env;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{HxError, HxResult};

use super::{RemoteFingerprint, RemoteSave, RemoteStat, RemoteTarget};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) struct FtpBackend {
    target: RemoteTarget,
    readonly: bool,
    client: FtpClient,
}

struct FtpSave {
    target: RemoteTarget,
    expected: Option<RemoteFingerprint>,
    client: FtpClient,
    data: Option<TcpStream>,
    temp_path: String,
}

struct FtpClient {
    reader: BufReader<TcpStream>,
}

#[derive(Debug)]
struct FtpResponse {
    code: u16,
    lines: Vec<String>,
}

impl fmt::Debug for FtpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FtpBackend")
            .field("target", &self.target.label())
            .field("readonly", &self.readonly)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for FtpSave {
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
        }))
    }

    pub(crate) fn reload(&mut self) -> HxResult<RemoteStat> {
        stat_with(&mut self.client, &self.target, self.readonly)
    }
}

impl Write for FtpSave {
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

impl RemoteSave for FtpSave {
    fn finish(mut self: Box<Self>) -> HxResult<RemoteStat> {
        if let Some(mut data) = self.data.take() {
            data.flush().map_err(remote_io)?;
            drop(data);
        }
        self.client.expect_transfer_complete("store")?;

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
        stat_with(&mut self.client, &self.target, false)
    }
}

impl FtpClient {
    fn connect(target: &RemoteTarget) -> HxResult<Self> {
        let port = target.port().unwrap_or(21);
        let stream = connect_tcp(target.host(), port)?;
        stream.set_read_timeout(Some(CONTROL_TIMEOUT)).ok();
        stream.set_write_timeout(Some(CONTROL_TIMEOUT)).ok();
        let mut client = Self {
            reader: BufReader::new(stream),
        };
        let welcome = client.read_response()?;
        expect_codes(&welcome, &[220], "connect")?;

        let (username, password) = ftp_credentials(target)?;
        let user = client.command("USER", Some(&username))?;
        match user.code {
            230 => {}
            331 | 332 => {
                let pass = client.command("PASS", Some(&password))?;
                expect_codes(&pass, &[230, 202], "login")?;
            }
            _ => return Err(response_error("login", &user)),
        }

        let typ = client.command("TYPE", Some("I"))?;
        expect_codes(&typ, &[200], "set binary mode")?;
        Ok(client)
    }

    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        let mut data = self.open_data_stream()?;
        if offset > 0 {
            let rest = self.command("REST", Some(&offset.to_string()))?;
            expect_codes(&rest, &[350], "restart transfer")?;
        }
        let retr = self.command("RETR", Some(path))?;
        expect_preliminary(&retr, "retrieve")?;

        let mut out = Vec::with_capacity(len);
        {
            let mut limited = Read::by_ref(&mut data).take(len as u64);
            limited.read_to_end(&mut out).map_err(remote_io)?;
        }
        drop(data);

        let completion = self.read_response()?;
        if completion.code == 226 || completion.code == 250 {
            return Ok(out);
        }
        if completion.code == 426 && out.len() == len {
            return Ok(out);
        }
        Err(response_error("retrieve", &completion))
    }

    fn begin_store(&mut self, path: &str) -> HxResult<TcpStream> {
        let data = self.open_data_stream()?;
        let stor = self.command("STOR", Some(path))?;
        expect_preliminary(&stor, "store")?;
        Ok(data)
    }

    fn expect_transfer_complete(&mut self, operation: &str) -> HxResult<()> {
        let completion = self.read_response()?;
        expect_codes(&completion, &[226, 250], operation)
    }

    fn stat_path(&mut self, path: &str, readonly: bool) -> HxResult<RemoteStat> {
        let size = self.command("SIZE", Some(path))?;
        expect_codes(&size, &[213], "size")?;
        let len = response_arg(&size)
            .parse::<u64>()
            .map_err(|err| HxError::Remote(format!("invalid FTP SIZE response: {err}")))?;

        let mdtm = match self.command("MDTM", Some(path)) {
            Ok(response) if response.code == 213 => {
                let value = response_arg(&response);
                if value.is_empty() {
                    None
                } else {
                    Some(value)
                }
            }
            Ok(_) | Err(_) => None,
        };

        Ok(RemoteStat {
            len,
            fingerprint: Some(RemoteFingerprint {
                len,
                mtime: None,
                file_id: mdtm.map(|value| format!("mdtm:{value}")),
            }),
            readonly,
            permissions: None,
        })
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> HxResult<()> {
        let rnfr = self.command("RNFR", Some(old_path))?;
        expect_codes(&rnfr, &[350], "rename-from")?;
        let rnto = self.command("RNTO", Some(new_path))?;
        expect_codes(&rnto, &[250], "rename-to")
    }

    fn remove(&mut self, path: &str) -> HxResult<()> {
        let response = self.command("DELE", Some(path))?;
        expect_codes(&response, &[250], "delete")
    }

    fn open_data_stream(&mut self) -> HxResult<TcpStream> {
        if let Ok(response) = self.command("EPSV", None) {
            if response.code == 229 {
                let port = parse_epsv_port(&response.message())?;
                let peer = self.reader.get_ref().peer_addr().map_err(remote_io)?;
                return connect_socket(SocketAddr::new(peer.ip(), port));
            }
        }

        let response = self.command("PASV", None)?;
        expect_codes(&response, &[227], "passive mode")?;
        let (host, port) = parse_pasv_endpoint(&response.message())?;
        connect_tcp(&host, port)
    }

    fn command(&mut self, command: &str, arg: Option<&str>) -> HxResult<FtpResponse> {
        validate_ftp_arg(command)?;
        if let Some(arg) = arg {
            validate_ftp_arg(arg)?;
        }
        let stream = self.reader.get_mut();
        stream.write_all(command.as_bytes()).map_err(remote_io)?;
        if let Some(arg) = arg {
            stream.write_all(b" ").map_err(remote_io)?;
            stream.write_all(arg.as_bytes()).map_err(remote_io)?;
        }
        stream.write_all(b"\r\n").map_err(remote_io)?;
        stream.flush().map_err(remote_io)?;
        self.read_response()
    }

    fn read_response(&mut self) -> HxResult<FtpResponse> {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).map_err(remote_io)?;
        if read == 0 {
            return Err(HxError::Remote("FTP control connection closed".to_owned()));
        }
        trim_line_end(&mut line);
        let code = parse_response_code(&line)?;
        let multiline = line.as_bytes().get(3) == Some(&b'-');
        let mut lines = vec![line];
        if multiline {
            loop {
                let mut next = String::new();
                let read = self.reader.read_line(&mut next).map_err(remote_io)?;
                if read == 0 {
                    return Err(HxError::Remote(
                        "FTP control connection closed during multiline response".to_owned(),
                    ));
                }
                trim_line_end(&mut next);
                let done = next.starts_with(&format!("{code:03} "));
                lines.push(next);
                if done {
                    break;
                }
            }
        }
        Ok(FtpResponse { code, lines })
    }
}

impl Drop for FtpClient {
    fn drop(&mut self) {
        let _ = self.command("QUIT", None);
    }
}

impl FtpResponse {
    fn message(&self) -> String {
        self.lines
            .iter()
            .map(|line| {
                if line.len() > 4 {
                    &line[4..]
                } else {
                    line.as_str()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
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

fn connect_socket(addr: SocketAddr) -> HxResult<TcpStream> {
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(remote_io)?;
    stream.set_read_timeout(Some(CONTROL_TIMEOUT)).ok();
    stream.set_write_timeout(Some(CONTROL_TIMEOUT)).ok();
    Ok(stream)
}

fn parse_response_code(line: &str) -> HxResult<u16> {
    if line.len() < 3 || !line.as_bytes()[..3].iter().all(u8::is_ascii_digit) {
        return Err(HxError::Remote(format!("invalid FTP response: {line}")));
    }
    line[..3]
        .parse::<u16>()
        .map_err(|err| HxError::Remote(format!("invalid FTP response code: {err}")))
}

fn response_arg(response: &FtpResponse) -> String {
    response
        .lines
        .last()
        .map(|line| {
            if line.len() > 4 {
                line[4..].trim().to_owned()
            } else {
                String::new()
            }
        })
        .unwrap_or_default()
}

fn expect_preliminary(response: &FtpResponse, operation: &str) -> HxResult<()> {
    if response.code / 100 == 1 {
        Ok(())
    } else {
        Err(response_error(operation, response))
    }
}

fn expect_codes(response: &FtpResponse, expected: &[u16], operation: &str) -> HxResult<()> {
    if expected.contains(&response.code) {
        Ok(())
    } else {
        Err(response_error(operation, response))
    }
}

fn response_error(operation: &str, response: &FtpResponse) -> HxError {
    HxError::Remote(format!(
        "FTP {operation} failed with status {}: {}",
        response.code,
        response.message()
    ))
}

fn parse_epsv_port(message: &str) -> HxResult<u16> {
    let start = message
        .find('(')
        .ok_or_else(|| HxError::Remote(format!("invalid EPSV response: {message}")))?;
    let end = message[start + 1..]
        .find(')')
        .ok_or_else(|| HxError::Remote(format!("invalid EPSV response: {message}")))?
        + start
        + 1;
    let inner = &message[start + 1..end];
    let delimiter = inner
        .chars()
        .next()
        .ok_or_else(|| HxError::Remote(format!("invalid EPSV response: {message}")))?;
    let fields: Vec<&str> = inner.split(delimiter).collect();
    fields
        .get(3)
        .ok_or_else(|| HxError::Remote(format!("invalid EPSV response: {message}")))?
        .parse::<u16>()
        .map_err(|err| HxError::Remote(format!("invalid EPSV port: {err}")))
}

fn parse_pasv_endpoint(message: &str) -> HxResult<(String, u16)> {
    let inner = match (message.find('('), message.rfind(')')) {
        (Some(start), Some(end)) if end > start => &message[start + 1..end],
        _ => message,
    };
    let numbers = inner
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u16>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| HxError::Remote(format!("invalid PASV response: {err}")))?;
    if numbers.len() < 6 {
        return Err(HxError::Remote(format!("invalid PASV response: {message}")));
    }
    let numbers = &numbers[numbers.len() - 6..];
    let host = format!(
        "{}.{}.{}.{}",
        numbers[0], numbers[1], numbers[2], numbers[3]
    );
    let port = numbers[4]
        .checked_mul(256)
        .and_then(|high| high.checked_add(numbers[5]))
        .ok_or_else(|| HxError::Remote(format!("invalid PASV port: {message}")))?;
    Ok((host, port))
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

fn trim_line_end(line: &mut String) {
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
}

fn remote_io(err: impl std::fmt::Display) -> HxError {
    HxError::Remote(err.to_string())
}
