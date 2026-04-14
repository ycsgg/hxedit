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
