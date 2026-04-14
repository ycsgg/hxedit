use std::fs;
use std::time::Instant;

use hxedit::config::Config;
use hxedit::core::document::Document;
use tempfile::tempdir;

#[test]
fn save_16mb_with_insert_is_fast() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("big.bin");

    let size: usize = 16 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let config = Config {
        page_size: 16384,
        cache_pages: 128,
        ..Config::default()
    };
    let mut doc = Document::open(&path, &config).unwrap();

    // Insert 2 bytes in the middle
    let mid = size as u64 / 2;
    doc.insert_bytes(mid, &[0xAA, 0xBB]).unwrap();

    let start = Instant::now();
    doc.save(None).unwrap();
    let elapsed = start.elapsed();

    eprintln!("16MB save took: {elapsed:?}");

    // Must complete in under 2 seconds (was ~30s before)
    assert!(
        elapsed.as_secs() < 2,
        "save took too long: {elapsed:?}"
    );

    // Verify correctness
    let saved = fs::read(&path).unwrap();
    assert_eq!(saved.len(), size + 2);
    // First half unchanged
    assert_eq!(&saved[..mid as usize], &data[..mid as usize]);
    // Inserted bytes
    assert_eq!(saved[mid as usize], 0xAA);
    assert_eq!(saved[mid as usize + 1], 0xBB);
    // Second half shifted
    assert_eq!(&saved[mid as usize + 2..], &data[mid as usize..]);
}

#[test]
fn save_16mb_with_tombstone_and_insert() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("big2.bin");

    let size: usize = 16 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let config = Config {
        page_size: 16384,
        cache_pages: 128,
        ..Config::default()
    };
    let mut doc = Document::open(&path, &config).unwrap();

    // Tombstone-delete byte at offset 100
    doc.delete_byte(100).unwrap();
    // Insert 2 bytes at offset 200
    doc.insert_bytes(200, &[0xCC, 0xDD]).unwrap();

    let start = Instant::now();
    doc.save(None).unwrap();
    let elapsed = start.elapsed();

    eprintln!("16MB save (tombstone+insert) took: {elapsed:?}");
    assert!(elapsed.as_secs() < 2, "save took too long: {elapsed:?}");

    let saved = fs::read(&path).unwrap();
    // Original 16MB - 1 tombstone + 2 inserted = 16MB + 1
    assert_eq!(saved.len(), size + 1);
}
