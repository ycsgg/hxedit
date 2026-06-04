use std::fs;
use std::path::Path;

use hxedit::config::Config;
use hxedit::core::document::Document;
use hxedit::mode::NibblePhase;
use tempfile::tempdir;

fn open_fixture(path: &str) -> Document {
    Document::open(Path::new(path), &Config::default()).unwrap()
}

fn open_temp(data: &[u8]) -> (tempfile::TempDir, Document) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("search.bin");
    fs::write(&path, data).unwrap();
    let doc = Document::open(&path, &Config::default()).unwrap();
    (dir, doc)
}

#[test]
fn searches_ascii_forward() {
    let mut doc = open_fixture("tests/fixtures/mixed.bin");
    assert_eq!(doc.search_forward(0, b"hello").unwrap(), Some(14));
}

#[test]
fn searches_ascii_backward() {
    let mut doc = open_fixture("tests/fixtures/mixed.bin");
    assert_eq!(
        doc.search_backward(doc.original_len(), b"hello").unwrap(),
        Some(14)
    );
}

#[test]
fn searches_hex_with_replacements() {
    let mut doc = open_fixture("tests/fixtures/mixed.bin");
    doc.replace_nibble(1, NibblePhase::High, 0x4).unwrap();
    doc.replace_nibble(1, NibblePhase::Low, 0x1).unwrap();
    assert_eq!(
        doc.search_forward(0, &[0x7f, 0x41, 0x4c, 0x46]).unwrap(),
        Some(0)
    );
}

#[test]
fn deleted_byte_breaks_match() {
    let mut doc = open_fixture("tests/fixtures/mixed.bin");
    doc.delete_byte(14).unwrap();
    assert_eq!(doc.search_forward(0, b"hello").unwrap(), None);
    assert_eq!(
        doc.search_backward(doc.original_len(), b"hello").unwrap(),
        None
    );
}

#[test]
fn searches_across_piece_boundaries() {
    let (_dir, mut doc) = open_temp(b"abef");
    doc.insert_bytes(2, b"cd").unwrap();

    assert_eq!(doc.search_forward(0, b"bcde").unwrap(), Some(1));
    assert_eq!(doc.search_backward(doc.len(), b"bcde").unwrap(), Some(1));
}

#[test]
fn searches_across_large_chunk_boundary_with_replacements() {
    let mut data = vec![b'x'; 70_000];
    let start = 65_534usize;
    data[start..start + 5].copy_from_slice(b"hxllo");

    let (_dir, mut doc) = open_temp(&data);
    doc.replace_display_byte(start as u64 + 1, b'e').unwrap();

    assert_eq!(doc.search_forward(0, b"hello").unwrap(), Some(start as u64));
    assert_eq!(
        doc.search_backward(doc.len(), b"hello").unwrap(),
        Some(start as u64)
    );
}

#[test]
fn clean_memmem_scan_finds_match_straddling_read_chunk() {
    // Clean documents take the SIMD memmem path. With the default config the
    // per-read chunk caps at page_size * cache_pages (2 MiB), so place a match
    // straddling that boundary to exercise the `pattern.len() - 1` overlap.
    let chunk = 16 * 1024 * 128; // 2 MiB
    let size = chunk + 4096;
    let mut data = vec![b'x'; size];
    let start = chunk - 2; // 2 bytes before the boundary, 3 after
    data[start..start + 5].copy_from_slice(b"hello");

    let (_dir, mut doc) = open_temp(&data);
    assert!(!doc.is_dirty());

    assert_eq!(doc.search_forward(0, b"hello").unwrap(), Some(start as u64));
    assert_eq!(
        doc.search_backward(doc.len(), b"hello").unwrap(),
        Some(start as u64)
    );
    // A pattern absent from the document still returns None across both paths.
    assert_eq!(doc.search_forward(0, b"zzzzz").unwrap(), None);
    assert_eq!(doc.search_backward(doc.len(), b"zzzzz").unwrap(), None);
}

#[test]
fn clean_memmem_scan_respects_start_and_direction() {
    // Two occurrences: forward from just past the first should find the
    // second; backward from before the second should find the first.
    let mut data = vec![b'.'; 4096];
    data[100..104].copy_from_slice(b"abcd");
    data[3000..3004].copy_from_slice(b"abcd");

    let (_dir, mut doc) = open_temp(&data);
    assert!(!doc.is_dirty());

    assert_eq!(doc.search_forward(0, b"abcd").unwrap(), Some(100));
    assert_eq!(doc.search_forward(101, b"abcd").unwrap(), Some(3000));
    assert_eq!(doc.search_backward(doc.len(), b"abcd").unwrap(), Some(3000));
    assert_eq!(doc.search_backward(3000, b"abcd").unwrap(), Some(100));
}

#[test]
fn clean_memmem_scan_handles_match_at_eof_and_bof() {
    let mut data = vec![b'-'; 8192];
    data[0..3].copy_from_slice(b"AAA");
    let last = data.len() - 3;
    data[last..].copy_from_slice(b"ZZZ");

    let (_dir, mut doc) = open_temp(&data);
    assert!(!doc.is_dirty());

    assert_eq!(doc.search_forward(0, b"AAA").unwrap(), Some(0));
    assert_eq!(doc.search_forward(0, b"ZZZ").unwrap(), Some(last as u64));
    assert_eq!(
        doc.search_backward(doc.len(), b"ZZZ").unwrap(),
        Some(last as u64)
    );
    assert_eq!(doc.search_backward(doc.len(), b"AAA").unwrap(), Some(0));
}
