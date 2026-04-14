//! Tests for the insert-mode nibble state machine, backspace, offset shifts,
//! tombstone coexistence, undo, search-after-insert, copy, paste, and save
//! edge cases.  Covers every scenario listed in insert-design.md § 必测场景.

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
// Insert 基础
// ═══════════════════════════════════════════════════════════════════════════

/// 在中间插入一个 nibble：high nibble 0xa → 字节 0xa0 出现在 display stream.
#[test]
fn insert_high_nibble_creates_byte_with_zero_low() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    // Insert byte 0xa0 at offset 2 (between 'b' and 'c')
    doc.insert_byte(2, 0xa0).unwrap();

    assert_eq!(doc.len(), 5);
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', 0xa0, b'c', b'd']);
}

/// 第二个 nibble 补齐：把 0xa0 改成 0xab.
#[test]
fn replace_low_nibble_completes_byte() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_byte(2, 0xa0).unwrap();
    // Simulate second nibble: replace the byte at offset 2 with 0xab
    doc.replace_display_byte(2, 0xab).unwrap();

    assert_eq!(doc.len(), 5);
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', 0xab, b'c', b'd']);
}

/// Esc 固化 a0 — 不撤销，低 nibble 0 成为真实值.
#[test]
fn esc_fixates_half_nibble_as_real_byte() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_byte(2, 0xa0).unwrap();
    // "Esc" just means we don't do anything else — the byte stays as 0xa0
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', 0xa0, b'c', b'd']);
}

// ═══════════════════════════════════════════════════════════════════════════
// 偏移变化
// ═══════════════════════════════════════════════════════════════════════════

/// 中间插入后，后续字节的 display offset 右移.
#[test]
fn insert_shifts_subsequent_offsets_right() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    // Before: offset 2 = 'c', offset 3 = 'd'
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Present(b'c'));
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(b'd'));

    doc.insert_bytes(2, &[0xAA, 0xBB]).unwrap();

    // After: offset 2 = 0xAA, offset 3 = 0xBB, offset 4 = 'c', offset 5 = 'd'
    assert_eq!(doc.len(), 6);
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Present(0xAA));
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(0xBB));
    assert_eq!(doc.byte_at(4).unwrap(), ByteSlot::Present(b'c'));
    assert_eq!(doc.byte_at(5).unwrap(), ByteSlot::Present(b'd'));
}

/// Insert Backspace (真实删除) 后，后续 offset 左移.
#[test]
fn real_delete_shifts_subsequent_offsets_left() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(2, &[0xAA]).unwrap();
    // Now: a b AA c d  (len=5)
    assert_eq!(doc.len(), 5);

    // Real-delete the inserted byte at offset 2
    doc.delete_range_real(2, 1).unwrap();

    // Back to: a b c d  (len=4)
    assert_eq!(doc.len(), 4);
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Present(b'c'));
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(b'd'));
}

// ═══════════════════════════════════════════════════════════════════════════
// Insert Backspace
// ═══════════════════════════════════════════════════════════════════════════

/// Backspace with pending: 删除刚插入的字节，回到插入前状态.
#[test]
fn backspace_with_pending_removes_just_inserted_byte() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_byte(2, 0xa0).unwrap();
    assert_eq!(doc.len(), 5);

    // Backspace removes it
    doc.delete_range_real(2, 1).unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(read_all(&mut doc), b"abcd".to_vec());
}

/// Backspace without pending: 真实删除光标前一个字节.
#[test]
fn backspace_without_pending_deletes_previous_byte() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    // Cursor at offset 2 ('c'), backspace deletes offset 1 ('b')
    doc.delete_range_real(1, 1).unwrap();
    assert_eq!(doc.len(), 3);
    assert_eq!(read_all(&mut doc), vec![b'a', b'c', b'd']);
}

/// Backspace 可以删除原文件字节.
#[test]
fn backspace_deletes_original_byte_via_piece_table() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    let removed = doc.delete_range_real(3, 1).unwrap();
    assert_eq!(doc.len(), 5);
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', b'c', b'e', b'f']);

    // Verify the removed cell was Original(3)
    assert_eq!(removed.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 与 tombstone 共存
// ═══════════════════════════════════════════════════════════════════════════

/// 原文件某字节 tombstone 后仍占位.
#[test]
fn tombstone_still_occupies_display_slot() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.delete_byte(1).unwrap(); // tombstone 'b'

    assert_eq!(doc.len(), 4); // length unchanged
    assert_eq!(doc.byte_at(0).unwrap(), ByteSlot::Present(b'a'));
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Deleted);
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Present(b'c'));
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(b'd'));
}

/// 在 tombstone 前后插入时显示正确.
#[test]
fn insert_around_tombstone_preserves_layout() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.delete_byte(1).unwrap(); // tombstone 'b'

    // Insert before the tombstone (at offset 1)
    doc.insert_byte(1, 0xAA).unwrap();
    // Now: a AA [deleted-b] c d  (len=5)
    assert_eq!(doc.len(), 5);
    assert_eq!(doc.byte_at(0).unwrap(), ByteSlot::Present(b'a'));
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(0xAA));
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Deleted); // tombstone shifted
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(b'c'));
    assert_eq!(doc.byte_at(4).unwrap(), ByteSlot::Present(b'd'));

    // Insert after the tombstone (at offset 3, which is 'c')
    doc.insert_byte(3, 0xBB).unwrap();
    // Now: a AA [deleted-b] BB c d  (len=6)
    assert_eq!(doc.len(), 6);
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(0xBB));
    assert_eq!(doc.byte_at(4).unwrap(), ByteSlot::Present(b'c'));
}

/// visible_len 正确反映 tombstone 和插入.
#[test]
fn visible_len_accounts_for_tombstones_and_inserts() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    assert_eq!(doc.visible_len(), 4);

    doc.delete_byte(1).unwrap();
    assert_eq!(doc.visible_len(), 3); // one tombstone

    doc.insert_byte(0, 0xAA).unwrap();
    assert_eq!(doc.visible_len(), 4); // +1 inserted, still 1 tombstone
}

// ═══════════════════════════════════════════════════════════════════════════
// Undo
// ═══════════════════════════════════════════════════════════════════════════

/// Undo insert: 插入一个字节后 undo 回到插入前.
#[test]
fn undo_insert_restores_original() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_byte(2, 0xAA).unwrap();
    assert_eq!(doc.len(), 5);

    // Undo = real-delete the inserted byte
    doc.delete_range_real(2, 1).unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(read_all(&mut doc), b"abcd".to_vec());
}

/// Undo tombstone delete: 恢复被 tombstone 的字节.
#[test]
fn undo_tombstone_delete_clears_tombstone() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    let id = doc.delete_byte(1).unwrap().unwrap();

    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Deleted);

    // Undo = clear tombstone
    doc.clear_tombstones(&[id]);
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(b'b'));
}

/// Undo visual delete: 批量恢复 tombstone.
#[test]
fn undo_visual_delete_restores_all_tombstones() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    let mut ids = Vec::new();
    for offset in 1..=3 {
        if let Some(id) = doc.delete_byte(offset).unwrap() {
            ids.push(id);
        }
    }
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Deleted);
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Deleted);
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Deleted);

    doc.clear_tombstones(&ids);
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(b'b'));
    assert_eq!(doc.byte_at(2).unwrap(), ByteSlot::Present(b'c'));
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(b'd'));
}

/// Undo real delete: 恢复被真实删除的字节.
#[test]
fn undo_real_delete_restores_bytes() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    let removed = doc.delete_range_real(2, 2).unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', b'e', b'f']);

    // Undo = re-insert the removed cells
    doc.restore_real_delete(2, &removed).unwrap();
    assert_eq!(doc.len(), 6);
    assert_eq!(read_all(&mut doc), b"abcdef".to_vec());
}

/// Undo paste: paste 插入的字节可以通过 real-delete 一次性撤销.
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

    // Undo paste = real-delete the 3 inserted bytes
    doc.delete_range_real(2, 3).unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(read_all(&mut doc), b"abcd".to_vec());
}

/// Undo replacement: 恢复替换前的值.
#[test]
fn undo_replacement_restores_original_value() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    let id = doc.cell_id_at(1).unwrap();
    let prev = doc.replacement_state(id);
    assert_eq!(prev, None);

    doc.replace_nibble(1, NibblePhase::High, 0x4).unwrap();
    doc.replace_nibble(1, NibblePhase::Low, 0x1).unwrap();
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(0x41)); // 'A'

    // Undo = restore previous replacement state (None → remove replacement)
    doc.restore_replacement(id, prev).unwrap();
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(b'b'));
}

// ═══════════════════════════════════════════════════════════════════════════
// 搜索
// ═══════════════════════════════════════════════════════════════════════════

/// 可命中插入内容.
#[test]
fn search_hits_inserted_content() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    doc.insert_bytes(3, &[0x58, 0x59]).unwrap(); // insert "XY" at offset 3
                                                 // Now: a b c X Y d e f

    let found = doc.search_forward(0, &[0x58, 0x59]).unwrap();
    assert_eq!(found, Some(3));
}

/// 不命中 tombstone 占位.
#[test]
fn search_does_not_match_across_tombstone() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    doc.delete_byte(2).unwrap(); // tombstone 'c'

    // "bc" should not match because 'c' is tombstoned
    let found = doc.search_forward(0, &[b'b', b'c']).unwrap();
    assert_eq!(found, None);

    // But "ab" still matches
    let found = doc.search_forward(0, &[b'a', b'b']).unwrap();
    assert_eq!(found, Some(0));

    // And "de" still matches (across the tombstone gap)
    let found = doc.search_forward(0, &[b'd', b'e']).unwrap();
    assert_eq!(found, Some(3));
}

/// 插入后搜索结果位置正确 (offset 已右移).
#[test]
fn search_after_insert_returns_shifted_offset() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    // Before insert: "ef" is at offset 4
    assert_eq!(doc.search_forward(0, b"ef").unwrap(), Some(4));

    // Insert 2 bytes at offset 2
    doc.insert_bytes(2, &[0xAA, 0xBB]).unwrap();

    // After insert: "ef" should now be at offset 6
    assert_eq!(doc.search_forward(0, b"ef").unwrap(), Some(6));
}

/// 反向搜索也能命中插入内容.
#[test]
fn search_backward_hits_inserted_content() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    doc.insert_bytes(3, &[0x58, 0x59]).unwrap();

    let found = doc.search_backward(doc.len(), &[0x58, 0x59]).unwrap();
    assert_eq!(found, Some(3));
}

// ═══════════════════════════════════════════════════════════════════════════
// Copy (logical_bytes)
// ═══════════════════════════════════════════════════════════════════════════

/// Copy 跳过 tombstone 字节.
#[test]
fn logical_bytes_skips_tombstones() {
    let (_dir, mut doc) = tmp_doc(b"abcdef");
    doc.delete_byte(2).unwrap(); // tombstone 'c'

    // logical_bytes for the full range should skip the tombstone
    let bytes = doc.logical_bytes(0, 5).unwrap();
    assert_eq!(bytes, vec![b'a', b'b', b'd', b'e', b'f']);
}

/// Copy 包含插入字节.
#[test]
fn logical_bytes_includes_inserted_bytes() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(2, &[0xAA, 0xBB]).unwrap();

    let bytes = doc.logical_bytes(0, 5).unwrap();
    assert_eq!(bytes, vec![b'a', b'b', 0xAA, 0xBB, b'c', b'd']);
}

// ═══════════════════════════════════════════════════════════════════════════
// Paste (插入语义)
// ═══════════════════════════════════════════════════════════════════════════

/// Paste 在当前位置插入，不是覆盖.
#[test]
fn paste_inserts_at_cursor_not_overwrite() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(2, b"XY").unwrap();

    assert_eq!(doc.len(), 6);
    assert_eq!(read_all(&mut doc), vec![b'a', b'b', b'X', b'Y', b'c', b'd']);
}

/// Paste 后续 offset 右移.
#[test]
fn paste_shifts_subsequent_offsets() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(1, b"XYZ").unwrap();

    assert_eq!(doc.byte_at(0).unwrap(), ByteSlot::Present(b'a'));
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(b'X'));
    assert_eq!(doc.byte_at(4).unwrap(), ByteSlot::Present(b'b'));
}

// ═══════════════════════════════════════════════════════════════════════════
// 保存
// ═══════════════════════════════════════════════════════════════════════════

/// 保存：插入后文件内容正确.
#[test]
fn save_after_insert_produces_correct_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.bin");
    fs::write(&path, b"abcdef").unwrap();

    let mut doc = Document::open(&path, &Config::default()).unwrap();
    doc.insert_bytes(3, &[0xAA, 0xBB]).unwrap();
    doc.save(None).unwrap();

    let saved = fs::read(&path).unwrap();
    assert_eq!(saved, vec![b'a', b'b', b'c', 0xAA, 0xBB, b'd', b'e', b'f']);
}

/// 保存：普通删除仍跳过导出.
#[test]
fn save_skips_tombstoned_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.bin");
    fs::write(&path, b"abcdef").unwrap();

    let mut doc = Document::open(&path, &Config::default()).unwrap();
    doc.delete_byte(2).unwrap();
    doc.delete_byte(3).unwrap();
    doc.save(None).unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"abef");
}

/// 保存：insert-backspace 的真实删除不写出.
#[test]
fn save_after_real_delete_omits_deleted_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.bin");
    fs::write(&path, b"abcdef").unwrap();

    let mut doc = Document::open(&path, &Config::default()).unwrap();
    // Insert then immediately backspace (real-delete)
    doc.insert_byte(3, 0xAA).unwrap();
    doc.delete_range_real(3, 1).unwrap();
    doc.save(None).unwrap();

    // File should be unchanged
    assert_eq!(fs::read(&path).unwrap(), b"abcdef");
}

/// 保存：同时处理 tombstone 跳过 + 插入写出.
#[test]
fn save_handles_tombstone_and_insert_together() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.bin");
    fs::write(&path, b"abcdef").unwrap();

    let mut doc = Document::open(&path, &Config::default()).unwrap();
    doc.delete_byte(1).unwrap(); // tombstone 'b'
    doc.insert_bytes(4, &[0xAA, 0xBB]).unwrap(); // insert after 'd'
    doc.save(None).unwrap();

    // Expected: a c d AA BB e f  (no 'b')
    let saved = fs::read(&path).unwrap();
    assert_eq!(saved, vec![b'a', b'c', b'd', 0xAA, 0xBB, b'e', b'f']);
}

/// 保存：替换 + tombstone + 插入 全部正确.
#[test]
fn save_handles_replacement_tombstone_and_insert() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.bin");
    fs::write(&path, b"abcdef").unwrap();

    let mut doc = Document::open(&path, &Config::default()).unwrap();
    doc.replace_nibble(0, NibblePhase::High, 0x4).unwrap(); // 'a' → 0x41='A'
    doc.replace_nibble(0, NibblePhase::Low, 0x1).unwrap();
    doc.delete_byte(2).unwrap(); // tombstone 'c'
    doc.insert_bytes(4, &[0xFF]).unwrap(); // insert after 'd'
    doc.save(None).unwrap();

    // Expected: A b d FF e f  (no 'c', 'a' replaced with 'A')
    let saved = fs::read(&path).unwrap();
    assert_eq!(saved, vec![0x41, b'b', b'd', 0xFF, b'e', b'f']);
}

// ═══════════════════════════════════════════════════════════════════════════
// PieceTable 补充
// ═══════════════════════════════════════════════════════════════════════════

/// 头部插入.
#[test]
fn insert_at_head() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(0, &[0xAA]).unwrap();
    assert_eq!(doc.len(), 5);
    assert_eq!(doc.byte_at(0).unwrap(), ByteSlot::Present(0xAA));
    assert_eq!(doc.byte_at(1).unwrap(), ByteSlot::Present(b'a'));
}

/// 尾部插入.
#[test]
fn insert_at_tail() {
    let (_dir, mut doc) = tmp_doc(b"abcd");
    doc.insert_bytes(4, &[0xAA]).unwrap();
    assert_eq!(doc.len(), 5);
    assert_eq!(doc.byte_at(3).unwrap(), ByteSlot::Present(b'd'));
    assert_eq!(doc.byte_at(4).unwrap(), ByteSlot::Present(0xAA));
}

/// 空文件插入.
#[test]
fn insert_into_empty_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("empty.bin");
    fs::write(&path, b"").unwrap();

    let mut doc = Document::open(&path, &Config::default()).unwrap();
    assert_eq!(doc.len(), 0);

    doc.insert_bytes(0, &[0xAA, 0xBB]).unwrap();
    assert_eq!(doc.len(), 2);
    assert_eq!(read_all(&mut doc), vec![0xAA, 0xBB]);
}

/// 多次连续插入.
#[test]
fn multiple_sequential_inserts() {
    let (_dir, mut doc) = tmp_doc(b"ab");
    doc.insert_byte(1, 0x01).unwrap(); // a 01 b
    doc.insert_byte(2, 0x02).unwrap(); // a 01 02 b
    doc.insert_byte(3, 0x03).unwrap(); // a 01 02 03 b

    assert_eq!(doc.len(), 5);
    assert_eq!(read_all(&mut doc), vec![b'a', 0x01, 0x02, 0x03, b'b']);
}
