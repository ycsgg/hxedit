//! Tests for insert-mode nibble state machine, backspace, tombstone coexistence,
//! undo, search, copy, paste, and save edge cases.
//!
//! Tests are consolidated by category to reduce total count while maintaining coverage.

use std::fs;

use hxedit::config::Config;
use hxedit::core::document::{ByteSlot, Document};
use hxedit::mode::NibblePhase;
use tempfile::tempdir;

// ─── helpers ────────────────────────────────────────────────────────────────

fn tmp_doc(data: &[u8]) -> (tempfile::TempDir, Document) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.bin");
    fs::write(&path, data).unwrap();
    let doc = Document::open(&path, &Config::default()).unwrap();
    (dir, doc)
}

fn read_all(doc: &mut Document) -> Vec<u8> {
    let mut out = Vec::new();
    for offset in 0..doc.len() {
        if let ByteSlot::Present(b) = doc.byte_at(offset).unwrap() {
            out.push(b);
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Insert basics: nibble editing and offset shifts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn nibble_edit_cycle_and_offset_shifts() {
    let (_dir, mut doc) = tmp_doc(b"abcd");

    // High nibble creates byte with zero low
    doc.insert_byte(2, 0xa0).unwrap();
    assert_eq!(doc.len(), 5);
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', 0xa0, b'c', b'd']);

    // Second nibble completes the byte
    doc.replace_display_byte(2, 0xab).unwrap();
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', 0xab, b'c', b'd']);

    // Insert shifts subsequent offsets right
    doc.insert_bytes(2, &[0xAA, 0xBB]).unwrap();
    assert_eq!(doc.len(), 7);
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Present(0xAA));
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(0xBB));
    assert_eq!(doc.byte_at(4).unwrap(), ByteSlot::Present(0xab));
}

#[test]
fn insert_at_boundaries() {
    // Head insert
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(0, &[0xAA]).unwrap();
    assert_eq!(doc.len(), 5);
    assert_eq!(doc.byte_at(0).unwrap(), ByteSlot::Present(0xAA));

    // Tail insert
    doc.insert_bytes(5, &[0xBB]).unwrap();
    assert_eq!(doc.len(), 6);
    assert_eq!(doc.byte_at(5).unwrap(), ByteSlot::Present(0xBB));

    // Empty file insert
    let dir = tempdir().unwrap();
    let path = dir.path().join("empty.bin");
    fs::write(&path, b"").unwrap();
    let mut doc2 = Document::open(&path, &Config::default()).unwrap();
    assert_eq!(doc2.len(), 0);
    doc2.insert_bytes(0, &[0xAA, 0xBB]).unwrap();
    assert_eq!(doc2.len(), 2);
    assert_eq!(read_all(&mut doc2), vec![0xAA, 0xBB]);

    // Multiple sequential inserts
    let (_dir, mut doc3) = tmp_doc(b"ab");
    doc3.insert_byte(1, 0x01).unwrap();
    doc3.insert_byte(2, 0x02).unwrap();
    doc3.insert_byte(3, 0x03).unwrap();
    assert_eq!(read_all(&mut doc3), vec![b'a', 0x01, 0x02, 0x03, b'b']);
}

// ═══════════════════════════════════════════════════════════════════════════
// Backspace: pending and non-pending cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backspace_various_cases() {
    // Backspace with pending removes just-inserted byte
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_byte(2, 0xa0).unwrap();
    assert_eq!(doc.len(), 5);
    doc.delete_range_real(2, 1).unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(read_all(&mut doc), b"abcd".to_vec());

    // Backspace without pending deletes previous byte
    let (_dir, mut doc2) = tmp_doc(b"abcd");
    doc2.delete_range_real(1, 1).unwrap();
    assert_eq!(doc2.len(), 3);
    assert_eq!(read_all(&mut doc2), vec![b'a', b'c', b'd']);

    // Real delete shifts subsequent offsets left
    let (_dir, mut doc3) = tmp_doc(b"abcdef");
    doc3.insert_bytes(2, &[0xAA]).unwrap();
    assert_eq!(doc3.len(), 7);
    doc3.delete_range_real(2, 1).unwrap();
    assert_eq!(doc3.len(), 6);
    assert_eq!(doc3.byte_at(2).unwrap(), ByteSlot::Present(b'c'));
}

// ═══════════════════════════════════════════════════════════════════════════
// Tombstone: display slot preservation and layout
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tombstone_preserves_display_slot_and_layout() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.delete_byte(1).unwrap(); // tombstone 'b'

    // Tombstone still occupies display slot
    assert_eq!(doc.len(), 4);
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Deleted);

    // Insert around tombstone preserves layout
    doc.insert_byte(1, 0xAA).unwrap();
    assert_eq!(doc.len(), 5);
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(0xAA));
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Deleted); // tombstone shifted

    doc.insert_byte(3, 0xBB).unwrap();
    assert_eq!(doc.len(), 6);
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(0xBB));

    // visible_len accounts for tombstones and inserts
    let (_dir, mut doc2) = tmp_doc(b"abcd");
    assert_eq!(doc2.visible_len(), 4);
    doc2.delete_byte(1).unwrap();
    assert_eq!(doc2.visible_len(), 3);
    doc2.insert_byte(0, 0xAA).unwrap();
    assert_eq!(doc2.visible_len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// Undo: insert, tombstone, real delete, replacement, paste
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn undo_various_operations() {
    // Undo insert
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_byte(2, 0xAA).unwrap();
    assert_eq!(doc.len(), 5);
    doc.delete_range_real(2, 1).unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(read_all(&mut doc), b"abcd".to_vec());

    // Undo tombstone delete
    let (_dir, mut doc2) = tmp_doc(b"abcd");
    let id = doc2.delete_byte(1).unwrap().unwrap();
    assert_eq!(doc2.byte_at(1).unwrap(), ByteSlot::Deleted);
    doc2.clear_tombstones(&[id]);
    assert_eq!(doc2.byte_at(1).unwrap(), ByteSlot::Present(b'b'));

    // Undo visual delete (batch tombstones)
    let (_dir, mut doc3) = tmp_doc(b"abcdef");
    let mut ids = Vec::new();
    for offset in 1..=3 {
        if let Some(id) = doc3.delete_byte(offset).unwrap() {
            ids.push(id);
        }
    }
    assert_eq!(doc3.byte_at(1).unwrap(), ByteSlot::Deleted);
    doc3.clear_tombstones(&ids);
    assert_eq!(doc3.byte_at(1).unwrap(), ByteSlot::Present(b'b'));
    assert_eq!(doc3.byte_at(2).unwrap(), ByteSlot::Present(b'c'));

    // Undo real delete
    let (_dir, mut doc4) = tmp_doc(b"abcdef");
    let removed = doc4.delete_range_real(2, 2).unwrap();
    assert_eq!(doc4.len(), 4);
    doc4.restore_real_delete(2, &removed).unwrap();
    assert_eq!(doc4.len(), 6);
    assert_eq!(read_all(&mut doc4), b"abcdef".to_vec());

    // Undo replacement
    let (_dir, mut doc5) = tmp_doc(b"abcd");
    let id = doc5.cell_id_at(1).unwrap();
    let prev = doc5.replacement_state(id);
    doc5.replace_nibble(1, NibblePhase::High, 0x4).unwrap();
    doc5.replace_nibble(1, NibblePhase::Low, 0x1).unwrap();
    assert_eq!(doc5.byte_at(1).unwrap(), ByteSlot::Present(0x41));
    doc5.restore_replacement(id, prev).unwrap();
    assert_eq!(doc5.byte_at(1).unwrap(), ByteSlot::Present(b'b'));
}

#[test]
fn undo_paste_removes_all_pasted_bytes() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    let paste_data = b"XYZ";
    doc.insert_bytes(2, paste_data).unwrap();
    assert_eq!(doc.len(), 7);
    assert_eq!(
        read_all(&mut doc),
        vec![b'a', b'b', b'X', b'Y', b'Z', b'c', b'd']
    );

    doc.delete_range_real(2, 3).unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(read_all(&mut doc), b"abcd".to_vec());
}

// ═══════════════════════════════════════════════════════════════════════════
// Search: forward, backward, with insert/tombstone
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn search_various_scenarios() {
    // Search hits inserted content
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    doc.insert_bytes(3, &[0x58, 0x59]).unwrap();
    let found = doc.search_forward(0, &[0x58, 0x59]).unwrap();
    assert_eq!(found, Some(3));

    // Search backward hits inserted content
    let found = doc.search_backward(doc.len(), &[0x58, 0x59]).unwrap();
    assert_eq!(found, Some(3));

    // Search does not match across tombstone
    let (_dir, mut doc2) = tmp_doc(b"abcdef");
    doc2.delete_byte(2).unwrap(); // tombstone 'c'
    let found = doc2.search_forward(0, b"bc").unwrap();
    assert_eq!(found, None); // 'c' is tombstoned
    let found = doc2.search_forward(0, b"ab").unwrap();
    assert_eq!(found, Some(0));
    let found = doc2.search_forward(0, b"de").unwrap();
    assert_eq!(found, Some(3)); // across tombstone gap

    // Search after insert returns shifted offset
    let (_dir, mut doc3) = tmp_doc(b"abcdef");
    assert_eq!(doc3.search_forward(0, b"ef").unwrap(), Some(4));
    doc3.insert_bytes(2, &[0xAA, 0xBB]).unwrap();
    assert_eq!(doc3.search_forward(0, b"ef").unwrap(), Some(6));
}

// ═══════════════════════════════════════════════════════════════════════════
// Copy (logical_bytes): skips tombstones, includes inserts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn logical_bytes_various_cases() {
    // Skips tombstones
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    doc.delete_byte(2).unwrap();
    let bytes = doc.logical_bytes(0, 5).unwrap();
    assert_eq!(bytes, vec![b'a', b'b', b'd', b'e', b'f']);

    // Includes inserted bytes
    let (_dir, mut doc2) = tmp_doc(b"abcd");
    doc2.insert_bytes(2, &[0xAA, 0xBB]).unwrap();
    let bytes = doc2.logical_bytes(0, 5).unwrap();
    assert_eq!(bytes, vec![b'a', b'b', 0xAA, 0xBB, b'c', b'd']);
}

// ═══════════════════════════════════════════════════════════════════════════
// Paste: insert semantics and offset shifts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn paste_inserts_at_cursor_not_overwrite() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(2, b"XY").unwrap();
    assert_eq!(doc.len(), 6);
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', b'X', b'Y', b'c', b'd']);

    doc.insert_bytes(1, b"XYZ").unwrap();
    assert_eq!(doc.byte_at(0).unwrap(), ByteSlot::Present(b'a'));
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(b'X'));
}

// ═══════════════════════════════════════════════════════════════════════════
// Save: insert, tombstone, real delete, replacement combinations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn save_various_edit_combinations() {
    // Save after insert
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.bin");
    fs::write(&path, b"abcdef").unwrap();
    let mut doc = Document::open(&path, &Config::default()).unwrap();
    doc.insert_bytes(3, &[0xAA, 0xBB]).unwrap();
    doc.save(None).unwrap();
    let saved = fs::read(&path).unwrap();
    assert_eq!(saved, vec![b'a', b'b', b'c', 0xAA, 0xBB, b'd', b'e', b'f']);

    // Save skips tombstoned bytes
    let dir2 = tempdir().unwrap();
    let path2 = dir2.path().join("test.bin");
    fs::write(&path2, b"abcdef").unwrap();
    let mut doc2 = Document::open(&path2, &Config::default()).unwrap();
    doc2.delete_byte(2).unwrap();
    doc2.delete_byte(3).unwrap();
    doc2.save(None).unwrap();
    assert_eq!(fs::read(&path2).unwrap(), b"abef");

    // Save after real delete omits deleted bytes
    let dir3 = tempdir().unwrap();
    let path3 = dir3.path().join("test.bin");
    fs::write(&path3, b"abcdef").unwrap();
    let mut doc3 = Document::open(&path3, &Config::default()).unwrap();
    doc3.insert_byte(3, 0xAA).unwrap();
    doc3.delete_range_real(3, 1).unwrap();
    doc3.save(None).unwrap();
    assert_eq!(fs::read(&path3).unwrap(), b"abcdef");

    // Save handles tombstone + insert together
    let dir4 = tempdir().unwrap();
    let path4 = dir4.path().join("test.bin");
    fs::write(&path4, b"abcdef").unwrap();
    let mut doc4 = Document::open(&path4, &Config::default()).unwrap();
    doc4.delete_byte(1).unwrap();
    doc4.insert_bytes(4, &[0xAA, 0xBB]).unwrap();
    doc4.save(None).unwrap();
    let saved = fs::read(&path4).unwrap();
    assert_eq!(saved, vec![b'a', b'c', b'd', 0xAA, 0xBB, b'e', b'f']);

    // Save handles replacement + tombstone + insert
    let dir5 = tempdir().unwrap();
    let path5 = dir5.path().join("test.bin");
    fs::write(&path5, b"abcdef").unwrap();
    let mut doc5 = Document::open(&path5, &Config::default()).unwrap();
    doc5.replace_nibble(0, NibblePhase::High, 0x4).unwrap();
    doc5.replace_nibble(0, NibblePhase::Low, 0x1).unwrap();
    doc5.delete_byte(2).unwrap();
    doc5.insert_bytes(4, &[0xFF]).unwrap();
    doc5.save(None).unwrap();
    let saved = fs::read(&path5).unwrap();
    assert_eq!(saved, vec![0x41, b'b', b'd', 0xFF, b'e', b'f']);
}
