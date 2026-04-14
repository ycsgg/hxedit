use std::fs;
use std::time::Instant;

use hxedit::config::Config;
use hxedit::core::document::Document;
use tempfile::tempdir;

#[test]
fn search_16mb_file_is_fast() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("big-search.bin");

    let size: usize = 16 * 1024 * 1024;
    let mut data = vec![0u8; size];
    let forward_needle = [0xde, 0xad, 0xbe, 0xef];
    let backward_needle = [0xca, 0xfe, 0xba, 0xbe];
    let forward_offset = size - forward_needle.len();
    let backward_offset = 128usize;
    data[forward_offset..forward_offset + forward_needle.len()].copy_from_slice(&forward_needle);
    data[backward_offset..backward_offset + backward_needle.len()]
        .copy_from_slice(&backward_needle);
    fs::write(&path, &data).unwrap();

    let config = Config {
        page_size: 16 * 1024,
        cache_pages: 128,
        ..Config::default()
    };
    let mut doc = Document::open(&path, &config).unwrap();

    let start = Instant::now();
    let forward = doc.search_forward(0, &forward_needle).unwrap();
    let forward_elapsed = start.elapsed();

    let start = Instant::now();
    let backward = doc.search_backward(doc.len(), &backward_needle).unwrap();
    let backward_elapsed = start.elapsed();

    eprintln!("16MB forward search took: {forward_elapsed:?}");
    eprintln!("16MB backward search took: {backward_elapsed:?}");

    assert_eq!(forward, Some(forward_offset as u64));
    assert_eq!(backward, Some(backward_offset as u64));
    assert!(
        forward_elapsed.as_secs() < 2,
        "forward search took too long: {forward_elapsed:?}"
    );
    assert!(
        backward_elapsed.as_secs() < 2,
        "backward search took too long: {backward_elapsed:?}"
    );
}
