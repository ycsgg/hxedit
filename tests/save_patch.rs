use std::fs;

use hxedit::config::Config;
use hxedit::core::document::Document;
use hxedit::mode::NibblePhase;
use tempfile::tempdir;

#[test]
fn saves_overwrite_in_place() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, b"abcdef").unwrap();

    let mut doc = Document::open(&file, &Config::default()).unwrap();
    doc.replace_nibble(1, NibblePhase::High, 0x3).unwrap();
    doc.replace_nibble(1, NibblePhase::Low, 0x1).unwrap();
    doc.replace_nibble(2, NibblePhase::High, 0x3).unwrap();
    doc.replace_nibble(2, NibblePhase::Low, 0x2).unwrap();
    doc.save(None).unwrap();

    assert_eq!(fs::read(&file).unwrap(), b"a12def");
}
