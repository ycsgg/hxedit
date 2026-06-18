use std::fs;
use std::path::{Path, PathBuf};

use tempfile::tempdir;

fn copy_example(dir: &Path, name: &str) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name);
    let target = dir.join(name);
    fs::copy(&source, &target).unwrap();
    target
}

fn run_hxedit(args: &[String]) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_hxedit"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn firmware_header_crc_macro_example_runs() {
    let dir = tempdir().unwrap();
    let macro_path = copy_example(dir.path(), "firmware_header_crc.hxmacro");
    let file = dir.path().join("firmware.bin");
    let mut sample = b"HXFW........".to_vec();
    sample.extend([0xff; 4]);
    sample.extend(b"0123456789abcdef0123456789abcdef");
    fs::write(&file, sample).unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--run".to_owned(),
        macro_path.display().to_string(),
    ]);

    let patched = fs::read(&file).unwrap();
    assert_eq!(&patched[..4], b"HXFW");
    assert_eq!(&patched[12..16], &[0, 0, 0, 0]);
    assert_eq!(&patched[patched.len() - 4..], b"HXFW");
}

#[test]
fn extract_selected_record_macro_example_runs() {
    let dir = tempdir().unwrap();
    let macro_path = copy_example(dir.path(), "extract_selected_record.hxmacro");
    let file = dir.path().join("record.bin");
    let encoded = [0xeb, 0xe8, 0xe9, 0xee, 0xef, 0xec, 0xed, 0xe2];
    let mut sample = vec![b'A'; 8];
    sample.extend(encoded);
    sample.extend(vec![b'B'; 48]);
    fs::write(&file, sample).unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--select".to_owned(),
        "display:8:8".to_owned(),
        "--run".to_owned(),
        macro_path.display().to_string(),
    ]);

    assert_eq!(
        fs::read(dir.path().join("selected-record.decoded.bin")).unwrap(),
        b"ABCDEFGH"
    );
    let audited = fs::read(dir.path().join("decoded-with-audit.bin")).unwrap();
    assert!(audited.len() > 64);
    assert!(audited[..64].iter().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn sanitize_log_copy_macro_example_runs() {
    let dir = tempdir().unwrap();
    let macro_path = copy_example(dir.path(), "sanitize_log_copy.hxmacro");
    let file = dir.path().join("log.bin");
    fs::write(&file, b"DEBUG password SECRET\r\n").unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--run".to_owned(),
        macro_path.display().to_string(),
    ]);

    assert_eq!(
        fs::read(dir.path().join("sanitized-log.bin")).unwrap(),
        b"INFO  password REDACT\n"
    );
    assert_eq!(fs::read(&file).unwrap(), b"DEBUG password SECRET\r\n");
}

#[test]
fn strip_debug_marker_macro_example_runs() {
    let dir = tempdir().unwrap();
    let macro_path = copy_example(dir.path(), "strip_debug_marker.hxmacro");
    let file = dir.path().join("debug.bin");
    fs::write(&file, b"payloadDEBUG_TRAILERtail").unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--run".to_owned(),
        macro_path.display().to_string(),
    ]);

    assert_eq!(
        fs::read(dir.path().join("without-debug-marker.bin")).unwrap(),
        b"payloadtail"
    );
}

#[cfg(feature = "scripting")]
#[test]
fn extract_payload_between_markers_script_example_runs() {
    let dir = tempdir().unwrap();
    let script = copy_example(dir.path(), "extract_payload_between_markers.hxscript");
    let file = dir.path().join("payload.bin");
    let mut sample = b"xxBEGIN_PAYLOAD\nhello\nEND_PAYLOAD".to_vec();
    sample.extend(vec![b'.'; 64]);
    fs::write(&file, sample).unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--script".to_owned(),
        script.display().to_string(),
    ]);

    assert_eq!(fs::read(dir.path().join("payload.bin")).unwrap(), b"hello");
    let sidecar = fs::read(dir.path().join("with-payload-sha256.bin")).unwrap();
    assert!(sidecar
        .windows(64)
        .any(|window| window.iter().all(|byte| byte.is_ascii_hexdigit())));
}

#[cfg(feature = "scripting")]
#[test]
fn decode_selected_xor_script_example_runs() {
    let dir = tempdir().unwrap();
    let script = copy_example(dir.path(), "decode_selected_xor.hxscript");
    let file = dir.path().join("record.bin");
    let encoded = [0xeb, 0xe8, 0xe9, 0xee, 0xef, 0xec, 0xed, 0xe2];
    fs::write(&file, encoded).unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--select".to_owned(),
        "display:0:8".to_owned(),
        "--script".to_owned(),
        script.display().to_string(),
    ]);

    assert_eq!(
        fs::read(dir.path().join("selection.after-xor.bin")).unwrap(),
        b"ABCDEFGH"
    );
    assert!(
        fs::read(dir.path().join("decoded-selection-with-audit.bin"))
            .unwrap()
            .starts_with(b"decoded-selection-space=display\n")
    );
}

#[cfg(feature = "scripting")]
#[test]
fn sanitize_log_copy_script_example_runs() {
    let dir = tempdir().unwrap();
    let script = copy_example(dir.path(), "sanitize_log_copy.hxscript");
    let file = dir.path().join("log.bin");
    fs::write(&file, b"DEBUG password SECRET\r\n").unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--script".to_owned(),
        script.display().to_string(),
    ]);

    assert_eq!(
        fs::read(dir.path().join("sanitized-log.bin")).unwrap(),
        b"INFO  ******** REDACT\n"
    );
}

#[cfg(feature = "scripting")]
#[test]
fn trim_debug_trailer_script_example_runs() {
    let dir = tempdir().unwrap();
    let script = copy_example(dir.path(), "trim_debug_trailer.hxscript");
    let file = dir.path().join("trim.bin");
    fs::write(&file, b"aaVOLATILEbbDEBUG_TRAILER").unwrap();

    run_hxedit(&[
        file.display().to_string(),
        "--script".to_owned(),
        script.display().to_string(),
    ]);

    assert_eq!(
        fs::read(dir.path().join("trimmed-debug-copy.bin")).unwrap(),
        b"aabb"
    );
}
