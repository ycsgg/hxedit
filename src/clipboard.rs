use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{HxError, HxResult};

pub fn copy_text(text: &str) -> HxResult<()> {
    #[cfg(target_os = "macos")]
    {
        return pipe_to_command("pbcopy", &[], text);
    }

    #[cfg(target_os = "windows")]
    {
        return pipe_to_command("clip", &[], text);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for (cmd, args) in [
            ("wl-copy", Vec::<&str>::new()),
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ] {
            if let Ok(()) = pipe_to_command(cmd, &args, text) {
                return Ok(());
            }
        }
        Err(HxError::Clipboard(
            "no clipboard tool found (tried wl-copy, xclip, xsel)".to_owned(),
        ))
    }
}

fn pipe_to_command(command: &str, args: &[&str], text: &str) -> HxResult<()> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| HxError::Clipboard(format!("{command}: {err}")))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|err| HxError::Clipboard(format!("{command}: {err}")))?;
    }

    let status = child
        .wait()
        .map_err(|err| HxError::Clipboard(format!("{command}: {err}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(HxError::Clipboard(format!(
            "{command} exited with status {status}"
        )))
    }
}
