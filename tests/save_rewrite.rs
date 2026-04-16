use std::fs;

use hxedit::config::Config;
use hxedit::core::document::Document;
use tempfile::tempdir;

#[test]
fn save_rewrites_file_when_deleted_bytes_exist() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, b"abcdef").unwrap();

    let mut doc = Document::open(&file, &Config::default()).unwrap();
    doc.delete_byte(2).unwrap();
    doc.delete_byte(3).unwrap();
    doc.save(None).unwrap();

    assert_eq!(fs::read(&file).unwrap(), b"abef");
}

#[test]
fn save_rewrites_file_when_appended_bytes_exist() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, b"abcdef").unwrap();

    let mut doc = Document::open(&file, &Config::default()).unwrap();
    doc.set_byte(6, b'X').unwrap();
    doc.set_byte(7, b'Y').unwrap();
    doc.save(None).unwrap();

    assert_eq!(fs::read(&file).unwrap(), b"abcdefXY");
}

#[cfg(unix)]
#[test]
fn save_rewrite_preserves_existing_permission_bits() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let file = dir.path().join("script.bin");
    fs::write(&file, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o751)).unwrap();

    let mut doc = Document::open(&file, &Config::default()).unwrap();
    doc.set_byte(2, b'/').unwrap();
    doc.save(None).unwrap();

    let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode, 0o751);
}
