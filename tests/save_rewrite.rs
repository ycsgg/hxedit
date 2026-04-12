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
