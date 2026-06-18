use std::fs;
use std::path::PathBuf;

use hxedit::cli::Cli;
use tempfile::tempdir;

fn base_cli(file: PathBuf) -> Cli {
    Cli {
        file: Some(file),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
        script: Vec::new(),
    }
}

#[test]
fn command_flags_execute_headlessly_and_save() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, b"abcd").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hxedit"))
        .arg(&file)
        .arg("--command")
        .arg("goto 0x1")
        .arg("--command")
        .arg("fill ff 2")
        .arg("--command")
        .arg("w")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&file).unwrap(), b"a\xff\xffd");
}

#[test]
fn run_macro_headlessly_can_reuse_hash_result_and_save() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    let macro_path = dir.path().join("patch.hxmacro");
    fs::write(&file, b"abcd........").unwrap();
    fs::write(
        &macro_path,
        r#"
version = 1
selection = "clear"

[[steps]]
cmd = "hash"
id = "payload_crc32"
algorithm = "crc32"
scope = { space = "display", start = "0x0", len = 4 }

[[steps]]
cmd = "overwrite"
offset = "0x4"
bytes = { from = "payload_crc32", format = "hex-text" }

[[steps]]
cmd = "save"
"#,
    )
    .unwrap();

    let mut cli = base_cli(file.clone());
    cli.run.push(macro_path);

    hxedit::headless::run(cli).unwrap();

    let mut expected = b"abcd".to_vec();
    let digest = crc32fast::hash(b"abcd").to_be_bytes();
    expected.extend(
        digest
            .iter()
            .flat_map(|byte| format!("{byte:02x}").into_bytes()),
    );
    assert_eq!(fs::read(&file).unwrap(), expected);
}

#[test]
fn select_applies_to_headless_export_and_xor_in_place() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    let export_path = dir.path().join("selection.bin");
    fs::write(&file, b"abcdef").unwrap();

    let mut cli = base_cli(file.clone());
    cli.select = Some("display:1:3".to_owned());
    cli.command = vec![
        format!("export {}", export_path.display()),
        "xor! 0xff".to_owned(),
        "w".to_owned(),
    ];

    hxedit::headless::run(cli).unwrap();

    assert_eq!(fs::read(&export_path).unwrap(), b"bcd");
    assert_eq!(
        fs::read(&file).unwrap(),
        vec![b'a', !b'b', !b'c', !b'd', b'e', b'f']
    );
}

#[cfg(feature = "scripting")]
#[test]
fn script_flag_executes_rhai_demo_and_saves() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(
        &file,
        [
            0xde, 0xad, 0xbe, 0xef, b'.', b'.', b'.', b'.', b'.', b'.', b'.', b'.',
        ],
    )
    .unwrap();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("simple_hash_patch.hxscript");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hxedit"))
        .arg(&file)
        .arg("--script")
        .arg(&script)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let digest = crc32fast::hash(&[0xde, 0xad, 0xbe, 0xef]).to_be_bytes();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut expected = vec![0xde, 0xad, 0xbe, 0xef];
    expected.extend_from_slice(hex.as_bytes());
    assert_eq!(fs::read(&file).unwrap(), expected);
}

#[cfg(feature = "scripting")]
#[test]
fn command_script_executes_rhai_then_command_save() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    let script = dir.path().join("patch.hxscript");
    fs::write(&file, b"abcd........").unwrap();
    fs::write(
        &script,
        r#"
hx_select_display(0, 4);
let digest = hx_hash_hex("crc32");
hx_overwrite(4, hx_ascii(digest));
"#,
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hxedit"))
        .arg(&file)
        .arg("--command")
        .arg(format!("script {}", script.display()))
        .arg("--command")
        .arg("w")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let digest = crc32fast::hash(b"abcd").to_be_bytes();
    let mut expected = b"abcd".to_vec();
    expected.extend(
        digest
            .iter()
            .flat_map(|byte| format!("{byte:02x}").into_bytes()),
    );
    assert_eq!(fs::read(&file).unwrap(), expected);
}

#[test]
fn headless_xor_requires_explicit_selection() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, b"abcd").unwrap();

    let mut cli = base_cli(file);
    cli.command.push("xor! 0xff".to_owned());

    let err = hxedit::headless::run(cli).unwrap_err();
    assert!(err.to_string().contains("active selection"));
}

#[test]
fn select_without_headless_actions_is_rejected_by_binary() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, b"abcd").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hxedit"))
        .arg("--select")
        .arg("display:0:1")
        .arg(&file)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--select requires --run"));
}
