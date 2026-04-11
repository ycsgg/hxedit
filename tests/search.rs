use std::path::Path;

use hxedit::config::Config;
use hxedit::core::document::Document;
use hxedit::mode::NibblePhase;

fn open_fixture(path: &str) -> Document {
    Document::open(Path::new(path), &Config::default()).unwrap()
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
