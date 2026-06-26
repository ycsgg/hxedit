use std::env;
use std::fmt;
use std::io::{self, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::error::{HxError, HxResult};

use super::{RemoteFingerprint, RemoteSave, RemoteStat, RemoteTarget};

const STDERR_LIMIT: usize = 16 * 1024;
const CONFLICT_MARKER: &str = "HXEDIT_REMOTE_CONFLICT";

const STAT_CODE: &str = r#"
import os, sys

def emit(path):
    st = os.stat(path)
    mode = st.st_mode & 0o7777
    mtime = getattr(st, "st_mtime_ns", int(st.st_mtime * 1000000000))
    file_id = f"dev:{st.st_dev}:ino:{st.st_ino}:mode:{mode:o}"
    print(f"{st.st_size}\t{mtime}\t{mode:o}\t{file_id}", flush=True)

emit(sys.argv[1])
"#;

const READ_CODE: &str = r#"
import sys

path = sys.argv[1]
offset = int(sys.argv[2])
length = int(sys.argv[3])
with open(path, "rb", buffering=0) as f:
    f.seek(offset)
    remaining = length
    out = sys.stdout.buffer
    while remaining > 0:
        chunk = f.read(min(1048576, remaining))
        if not chunk:
            break
        out.write(chunk)
        remaining -= len(chunk)
"#;

const WRITE_TEMP_CODE: &str = r#"
import os, sys

temp_path = sys.argv[1]
permissions = sys.argv[2]

fd = os.open(temp_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
try:
    with os.fdopen(fd, "wb") as f:
        while True:
            chunk = sys.stdin.buffer.read(1024 * 1024)
            if not chunk:
                break
            f.write(chunk)
        f.flush()
        try:
            os.fsync(f.fileno())
        except OSError:
            pass

    if permissions:
        os.chmod(temp_path, int(permissions, 8))
except Exception:
    try:
        os.unlink(temp_path)
    except OSError:
        pass
    raise
"#;

const COMMIT_TEMP_CODE: &str = r#"
import os, sys

CONFLICT_MARKER = "HXEDIT_REMOTE_CONFLICT"

path = sys.argv[1]
temp_path = sys.argv[2]
expected_len = sys.argv[3]
expected_mtime = sys.argv[4]
expected_file_id = sys.argv[5]

def stat_parts(target):
    st = os.stat(target)
    mode = st.st_mode & 0o7777
    mtime = getattr(st, "st_mtime_ns", int(st.st_mtime * 1000000000))
    file_id = f"dev:{st.st_dev}:ino:{st.st_ino}:mode:{mode:o}"
    return str(st.st_size), str(mtime), file_id, mode

def emit(target):
    size, mtime, file_id, mode = stat_parts(target)
    print(f"{size}\t{mtime}\t{mode:o}\t{file_id}", flush=True)

def cleanup_temp():
    try:
        os.unlink(temp_path)
    except OSError:
        pass

def fingerprint_matches():
    if not expected_len:
        return True
    size, mtime, file_id, _mode = stat_parts(path)
    return size == expected_len and mtime == expected_mtime and file_id == expected_file_id

try:
    if not fingerprint_matches():
        cleanup_temp()
        print(CONFLICT_MARKER, file=sys.stderr, flush=True)
        sys.exit(75)
    os.replace(temp_path, path)
    emit(path)
except Exception:
    cleanup_temp()
    raise
"#;

const REMOVE_TEMP_CODE: &str = r#"
import os, sys

try:
    os.unlink(sys.argv[1])
except FileNotFoundError:
    pass
"#;

pub(crate) struct OpenSshBackend {
    target: RemoteTarget,
    readonly: bool,
}

struct OpenSshSave {
    target: RemoteTarget,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
    expected: Option<RemoteFingerprint>,
    temp_path: String,
    completed: bool,
}

impl fmt::Debug for OpenSshBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenSshBackend")
            .field("target", &self.target.label())
            .field("readonly", &self.readonly)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for OpenSshSave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenSshSave")
            .field("target", &self.target.label())
            .field("temp_path", &self.temp_path)
            .finish_non_exhaustive()
    }
}

impl OpenSshBackend {
    pub(crate) fn open(target: RemoteTarget, readonly: bool) -> HxResult<Self> {
        validate_remote_arg(target.path())?;
        Ok(Self { target, readonly })
    }

    pub(crate) fn read_at(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        run_python_capture(
            &self.target,
            READ_CODE,
            &[self.target.path(), &offset.to_string(), &len.to_string()],
        )
    }

    pub(crate) fn begin_save(
        &self,
        expected: Option<RemoteFingerprint>,
    ) -> HxResult<Box<dyn RemoteSave>> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let current = stat_target(&self.target, self.readonly)?;
        if current.fingerprint != expected {
            return Err(HxError::RemoteConflict {
                target: self.target.label(),
            });
        }

        let save_fingerprint = current.fingerprint.clone();
        let permissions = current
            .permissions
            .map(|mode| format!("{mode:o}"))
            .unwrap_or_default();
        let temp_path = temp_path_for(self.target.path());
        let args = [temp_path.as_str(), permissions.as_str()];
        let mut child = python_command(&self.target, WRITE_TEMP_CODE, &args)
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
        Ok(Box::new(OpenSshSave {
            target: self.target.clone(),
            child,
            stdin: Some(stdin),
            stdout,
            stderr: drain_stderr(stderr),
            expected: save_fingerprint,
            temp_path,
            completed: false,
        }))
    }

    pub(crate) fn reload(&mut self) -> HxResult<RemoteStat> {
        stat_target(&self.target, self.readonly)
    }
}

impl Write for OpenSshSave {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("remote save stream is already closed"))?;
        stdin.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.flush()?;
        }
        Ok(())
    }
}

impl RemoteSave for OpenSshSave {
    fn finish(mut self: Box<Self>) -> HxResult<RemoteStat> {
        drop(self.stdin.take());

        let mut stdout = String::new();
        self.stdout
            .read_to_string(&mut stdout)
            .map_err(|err| HxError::Remote(format!("ssh save stdout read failed: {err}")))?;
        let status = self
            .child
            .wait()
            .map_err(|err| HxError::Remote(format!("ssh save wait failed: {err}")))?;

        if !status.success() {
            let stderr = stderr_message(&self.stderr);
            return Err(HxError::Remote(format!(
                "ssh temp upload failed{}{}",
                status
                    .code()
                    .map(|code| format!(" with exit code {code}"))
                    .unwrap_or_default(),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            )));
        }

        let (expected_len, expected_mtime, expected_file_id) =
            fingerprint_args(self.expected.as_ref());
        let output = run_python_capture_with_status(
            &self.target,
            COMMIT_TEMP_CODE,
            &[
                self.target.path(),
                &self.temp_path,
                &expected_len,
                &expected_mtime,
                &expected_file_id,
            ],
        );
        match output {
            Ok(stdout) => {
                self.completed = true;
                let stdout = String::from_utf8(stdout).map_err(|err| {
                    HxError::Remote(format!("ssh commit output was not UTF-8: {err}"))
                })?;
                parse_stat(&stdout, &self.target, false)
            }
            Err(SshCommandError::Conflict) => Err(HxError::RemoteConflict {
                target: self.target.label(),
            }),
            Err(SshCommandError::Remote(message)) => Err(HxError::Remote(message)),
        }
    }
}

impl Drop for OpenSshSave {
    fn drop(&mut self) {
        let _ = self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if !self.completed {
            let _ = run_python_capture(&self.target, REMOVE_TEMP_CODE, &[&self.temp_path]);
        }
    }
}

fn stat_target(target: &RemoteTarget, readonly: bool) -> HxResult<RemoteStat> {
    let stdout = run_python_capture(target, STAT_CODE, &[target.path()])?;
    let stdout = String::from_utf8(stdout)
        .map_err(|err| HxError::Remote(format!("ssh stat output was not UTF-8: {err}")))?;
    parse_stat(&stdout, target, readonly)
}

fn run_python_capture(target: &RemoteTarget, code: &str, args: &[&str]) -> HxResult<Vec<u8>> {
    run_python_capture_with_status(target, code, args).map_err(|err| match err {
        SshCommandError::Conflict => HxError::RemoteConflict {
            target: target.label(),
        },
        SshCommandError::Remote(message) => HxError::Remote(message),
    })
}

enum SshCommandError {
    Conflict,
    Remote(String),
}

fn run_python_capture_with_status(
    target: &RemoteTarget,
    code: &str,
    args: &[&str],
) -> Result<Vec<u8>, SshCommandError> {
    let output = python_command(target, code, args)
        .output()
        .map_err(|err| SshCommandError::Remote(format!("failed to start ssh: {err}")))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.contains(CONFLICT_MARKER) {
        return Err(SshCommandError::Conflict);
    }
    Err(SshCommandError::Remote(format!(
        "ssh command failed{}{}",
        output
            .status
            .code()
            .map(|code| format!(" with exit code {code}"))
            .unwrap_or_default(),
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    )))
}

fn python_command(target: &RemoteTarget, code: &str, args: &[&str]) -> Command {
    let mut command = ssh_command(target);
    command.arg(remote_python_command(code, args));
    command
}

fn ssh_command(target: &RemoteTarget) -> Command {
    let mut command = Command::new("ssh");
    command
        .arg("-T")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ClearAllForwardings=yes");
    if env::var_os("HXEDIT_SSH_INSECURE").is_some() {
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
    command.arg("--").arg(target.host());
    command
}

fn remote_python_command(code: &str, args: &[&str]) -> String {
    let mut words = Vec::with_capacity(args.len() + 3);
    words.push(shell_quote("python3"));
    words.push(shell_quote("-c"));
    words.push(shell_quote(code));
    for arg in args {
        words.push(shell_quote(arg));
    }
    words.join(" ")
}

fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn validate_remote_arg(value: &str) -> HxResult<()> {
    if value.contains('\0') {
        return Err(HxError::Remote(
            "ssh remote arguments must not contain NUL bytes".to_owned(),
        ));
    }
    Ok(())
}

fn parse_stat(output: &str, _target: &RemoteTarget, readonly: bool) -> HxResult<RemoteStat> {
    let line = output
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| HxError::Remote("ssh stat returned no output".to_owned()))?;
    let mut parts = line.split('\t');
    let len = parts
        .next()
        .ok_or_else(|| HxError::Remote("ssh stat missing length".to_owned()))?
        .parse::<u64>()
        .map_err(|err| HxError::Remote(format!("invalid ssh stat length: {err}")))?;
    let mtime = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);
    let permissions = parts
        .next()
        .filter(|value| !value.is_empty())
        .map(|value| {
            u32::from_str_radix(value, 8)
                .map_err(|err| HxError::Remote(format!("invalid ssh permissions: {err}")))
        })
        .transpose()?;
    let file_id = parts
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    Ok(RemoteStat {
        len,
        fingerprint: Some(RemoteFingerprint {
            len,
            mtime,
            file_id,
        }),
        readonly,
        permissions,
    })
}

fn fingerprint_args(fingerprint: Option<&RemoteFingerprint>) -> (String, String, String) {
    match fingerprint {
        Some(fingerprint) => (
            fingerprint.len.to_string(),
            fingerprint
                .mtime
                .map(|mtime| mtime.to_string())
                .unwrap_or_default(),
            fingerprint.file_id.clone().unwrap_or_default(),
        ),
        None => (String::new(), String::new(), String::new()),
    }
}

fn temp_path_for(path: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    match path.rsplit_once('/') {
        Some((dir, name)) if !name.is_empty() => {
            format!("{dir}/.{name}.hxedit.tmp.{stamp}")
        }
        _ => format!(".hxedit.tmp.{stamp}"),
    }
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

fn stderr_message(stderr: &Arc<Mutex<Vec<u8>>>) -> String {
    let Ok(stderr) = stderr.lock() else {
        return String::new();
    };
    String::from_utf8_lossy(&stderr).trim().to_owned()
}
