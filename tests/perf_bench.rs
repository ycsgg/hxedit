//! Performance benchmark harness — not a pass/fail test; emits numbers to stderr.
//!
//! Run with: `cargo test --release --test perf_bench -- --nocapture`

use std::fs;
use std::time::Instant;

use hxedit::config::Config;
use hxedit::core::document::Document;
use hxedit::format;
use tempfile::tempdir;

fn bench_config() -> Config {
    Config {
        page_size: 16384,
        cache_pages: 128,
        ..Config::default()
    }
}

fn print(label: &str, ns: u128, unit_count: usize) {
    let per = ns as f64 / unit_count.max(1) as f64;
    eprintln!("[bench] {label:<45} total {ns:>12} ns  per-op {per:>10.1} ns  (N={unit_count})");
}

#[test]
fn bench_resolve_piece_heavy() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("big.bin");
    let size: usize = 4 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let mut doc = Document::open(&path, &bench_config()).unwrap();
    // Create 5000 pieces by inserting 2 bytes every 800 bytes in first 4MB.
    for i in 0..5000u64 {
        let off = i * 800;
        if off < doc.len() {
            doc.insert_bytes(off + (i % 3), &[0xAA, 0xBB]).unwrap();
        }
    }

    let len = doc.len();
    let iters = 200_000;
    // Random-ish offsets across whole file
    let mut offs = Vec::with_capacity(iters);
    let mut x: u64 = 0x9E3779B97F4A7C15;
    for _ in 0..iters {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        offs.push(x % len);
    }

    let t = Instant::now();
    let mut hit = 0u64;
    for &o in &offs {
        if doc.cell_id_at(o).is_some() {
            hit += 1;
        }
    }
    let ns = t.elapsed().as_nanos();
    assert_eq!(hit as usize, iters);
    print("resolve random (4MB, ~5k pieces)", ns, iters);
}

#[test]
fn bench_save_16mb_with_insert() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s16.bin");
    let size: usize = 16 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let mut doc = Document::open(&path, &bench_config()).unwrap();
    doc.insert_bytes(size as u64 / 2, &[0xAA, 0xBB]).unwrap();
    let t = Instant::now();
    doc.save(None).unwrap();
    print("save 16MB clean+1 insert", t.elapsed().as_nanos(), 1);
}

#[test]
fn bench_save_16mb_with_tombstones() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s16t.bin");
    let size: usize = 16 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let mut doc = Document::open(&path, &bench_config()).unwrap();
    // Dense tombstones over a single 64KB range — forces slow path in one chunk.
    for i in 0..4096u64 {
        doc.delete_byte(1_000_000 + i).unwrap();
    }
    let t = Instant::now();
    doc.save(None).unwrap();
    print("save 16MB with 4096 tombstones", t.elapsed().as_nanos(), 1);
}

#[test]
fn bench_parse_elf_format() {
    // Use the test fixture ELF
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/elf_header.bin");
    if !path.exists() {
        eprintln!("[bench] parse_elf skipped: no fixture");
        return;
    }
    let mut doc = Document::open(&path, &bench_config()).unwrap();

    let t = Instant::now();
    let iters = 200;
    for _ in 0..iters {
        let det = format::detect::detect_format(&mut doc);
        if let Some(def) = det {
            let _ = format::parse::parse_format(&def, &mut doc).unwrap();
        }
    }
    print(
        "detect+parse ELF fixture",
        t.elapsed().as_nanos(),
        iters as usize,
    );
}

#[test]
fn bench_paste_overwrite_large() {
    // Simulate a large overwrite paste hitting apply_paste_overwrite path.
    // We don't go through App here (needs terminal); we exercise the underlying
    // Document calls that apply_paste_overwrite makes per-byte.
    let dir = tempdir().unwrap();
    let path = dir.path().join("p16.bin");
    let size: usize = 4 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();
    let mut doc = Document::open(&path, &bench_config()).unwrap();

    // Create a lot of pieces first
    for i in 0..2000u64 {
        doc.insert_bytes(i * 500, &[0xAA]).unwrap();
    }

    let n = 200_000usize;
    let bytes: Vec<u8> = (0..n).map(|i| (i % 256) as u8).collect();
    let t = Instant::now();
    // Mimic apply_paste_overwrite: per-byte cell_id_at + replace_display_byte.
    for (i, &b) in bytes.iter().enumerate() {
        let off = i as u64;
        if let Some(_id) = doc.cell_id_at(off) {
            let _ = doc.replace_display_byte(off, b);
        }
    }
    print(
        "paste overwrite 200k bytes into pieced doc",
        t.elapsed().as_nanos(),
        n,
    );
}

#[test]
fn bench_paste_overwrite_bulk_path() {
    // Measures the post-phase4 bulk overwrite: one cell_ids_range call +
    // replace_display_byte_by_id per cell, matching apply_paste_overwrite.
    let dir = tempdir().unwrap();
    let path = dir.path().join("p16b.bin");
    let size: usize = 4 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();
    let mut doc = Document::open(&path, &bench_config()).unwrap();
    for i in 0..2000u64 {
        doc.insert_bytes(i * 500, &[0xAA]).unwrap();
    }

    let n = 200_000usize;
    let bytes: Vec<u8> = (0..n).map(|i| (i % 256) as u8).collect();
    let t = Instant::now();
    let ids = doc.cell_ids_range(0, n as u64);
    for (b, id) in bytes.iter().copied().zip(ids.into_iter()) {
        let _ = doc.replace_display_byte_by_id(id, b);
    }
    print(
        "paste overwrite 200k bytes (bulk path)",
        t.elapsed().as_nanos(),
        n,
    );
}

#[test]
fn bench_logical_bytes_large_copy() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("c16.bin");
    let size: usize = 8 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();
    let mut doc = Document::open(&path, &bench_config()).unwrap();
    // Add some tombstones to force occasional slow path
    doc.delete_byte(100).unwrap();

    let t = Instant::now();
    let b = doc.logical_bytes(0, size as u64 - 1).unwrap();
    print("logical_bytes 8MB copy", t.elapsed().as_nanos(), 1);
    assert!(!b.is_empty());
}
