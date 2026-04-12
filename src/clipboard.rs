use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{HxError, HxResult};
use crate::util::parse::decode_base64;

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

pub fn read_text() -> HxResult<String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(String::from_utf8_lossy(&capture_command("pbpaste", &[])?).into_owned());
    }

    #[cfg(target_os = "windows")]
    {
        return Ok(String::from_utf8_lossy(&capture_command(
            "powershell",
            &["-NoProfile", "-Command", "Get-Clipboard"],
        )?)
        .into_owned());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for (cmd, args) in [
            ("wl-paste", Vec::<&str>::new()),
            ("xclip", vec!["-selection", "clipboard", "-o"]),
            ("xsel", vec!["--clipboard", "--output"]),
        ] {
            if let Ok(bytes) = capture_command(cmd, &args) {
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
        }
        Err(HxError::Clipboard(
            "no clipboard text reader found (tried wl-paste, xclip, xsel)".to_owned(),
        ))
    }
}

pub fn read_raw_bytes() -> HxResult<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(base64) = capture_command(
            "osascript",
            &["-l", "JavaScript", "-e", macos_raw_clipboard_script()],
        ) {
            let encoded = String::from_utf8_lossy(&base64).trim().to_owned();
            if !encoded.is_empty() {
                return decode_base64(&encoded).map_err(|err| HxError::Clipboard(err.to_string()));
            }
        }

        return capture_command("pbpaste", &[]);
    }

    #[cfg(target_os = "windows")]
    {
        return capture_command(
            "powershell",
            &["-NoProfile", "-Command", "Get-Clipboard -Raw"],
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for (cmd, args) in [
            ("wl-paste", vec!["--no-newline"]),
            ("xclip", vec!["-selection", "clipboard", "-o"]),
            ("xsel", vec!["--clipboard", "--output"]),
        ] {
            if let Ok(bytes) = capture_command(cmd, &args) {
                return Ok(bytes);
            }
        }
        Err(HxError::Clipboard(
            "no clipboard reader found (tried wl-paste, xclip, xsel)".to_owned(),
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

fn capture_command(command: &str, args: &[&str]) -> HxResult<Vec<u8>> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|err| HxError::Clipboard(format!("{command}: {err}")))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(HxError::Clipboard(format!(
            "{command} exited with status {}",
            output.status
        )))
    }
}

#[cfg(target_os = "macos")]
fn macos_raw_clipboard_script() -> &'static str {
    r#"ObjC.import("AppKit");
var pb = $.NSPasteboard.generalPasteboard;
var items = pb.pasteboardItems;
var preferred = [
  "public.png",
  "public.jpeg",
  "public.tiff",
  "com.compuserve.gif",
  "public.heic",
  "public.image"
];

function base64ForItem(item) {
  var types = ObjC.deepUnwrap(item.types);
  for (var p = 0; p < preferred.length; p++) {
    for (var i = 0; i < types.length; i++) {
      if (types[i] === preferred[p]) {
        var preferredData = item.dataForType(types[i]);
        if (preferredData && preferredData.length > 0) {
          return ObjC.unwrap(preferredData.base64EncodedStringWithOptions(0));
        }
      }
    }
  }

  for (var i = 0; i < types.length; i++) {
    var type = types[i];
    if (type.indexOf("text") !== -1 || type.indexOf("utf8") !== -1 || type.indexOf("html") !== -1) {
      continue;
    }
    var binaryData = item.dataForType(type);
    if (binaryData && binaryData.length > 0) {
      return ObjC.unwrap(binaryData.base64EncodedStringWithOptions(0));
    }
  }

  for (var i = 0; i < types.length; i++) {
    var data = item.dataForType(types[i]);
    if (data && data.length > 0) {
      return ObjC.unwrap(data.base64EncodedStringWithOptions(0));
    }
  }

  return "";
}

if (items && items.count > 0) {
  for (var i = 0; i < items.count; i++) {
    var encoded = base64ForItem(items.objectAtIndex(i));
    if (encoded.length > 0) {
      console.log(encoded);
      break;
    }
  }
}"#
}
