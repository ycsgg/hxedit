use std::fs;
use std::sync::Arc;

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

#[test]
fn save_rewrite_survives_ranges_larger_than_page_cache_capacity() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("small-cache.bin");
    let data = (0..70_000).map(|idx| (idx % 251) as u8).collect::<Vec<_>>();
    fs::write(&file, &data).unwrap();

    let config = Config {
        page_size: 256,
        cache_pages: 4,
        ..Config::default()
    };
    let mut doc = Document::open(&file, &config).unwrap();
    doc.replace_display_byte(10, 0xaa).unwrap();
    doc.save(None).unwrap();

    let mut expected = data;
    expected[10] = 0xaa;
    assert_eq!(fs::read(&file).unwrap(), expected);
}

#[test]
fn save_rewrites_range_overlay_replacements() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("overlay.bin");
    fs::write(&file, b"0123456789").unwrap();

    let mut doc = Document::open(&file, &Config::default()).unwrap();
    doc.overwrite_run_pattern_overlay(2, 5, b"ab").unwrap();
    doc.save(None).unwrap();

    assert_eq!(fs::read(&file).unwrap(), b"01ababa789");
}

#[test]
fn save_rewrites_bytes_overlay_replacements() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bytes-overlay.bin");
    fs::write(&file, b"0123456789").unwrap();

    let mut doc = Document::open(&file, &Config::default()).unwrap();
    doc.overwrite_run_bytes_overlay(2, Arc::from(&b"ABCDE"[..]))
        .unwrap();
    doc.save(None).unwrap();

    assert_eq!(fs::read(&file).unwrap(), b"01ABCDE789");
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
