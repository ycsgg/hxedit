//! Performance benchmark harness.
//!
//! Run with: `cargo bench --bench perf_bench`

use std::fs;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::process::Command;
use std::time::{Duration, Instant};

use digest::Digest;
use hxedit::commands::types::HashAlgorithm;
use hxedit::config::Config;
use hxedit::core::document::Document;
use hxedit::core::file_view::FileView;
use hxedit::diff::{find_mismatch_forward, find_mismatch_forward_step};
use hxedit::format;
use hxedit::mode::NibblePhase;
use tempfile::tempdir;

type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type BenchFn = fn() -> BenchResult;
type BenchEntry = (&'static str, BenchFn);

const BENCH_CHILD_ENV: &str = "HXEDIT_BENCH_CHILD";
const EDIT_FILE_SIZE: usize = 8 * 1024 * 1024;
const EDIT_SINGLE_OPS: usize = 200_000;
const EDIT_BULK_BYTES: usize = 1024 * 1024;
const EDIT_LARGE_FILE_SIZE: usize = 32 * 1024 * 1024;
const EDIT_LARGE_BULK_BYTES: usize = 16 * 1024 * 1024;
const EDIT_PER_BYTE_LARGE_BYTES: usize = 4 * 1024 * 1024;
const EDIT_256_FILE_SIZE: usize = 256 * 1024 * 1024;
const EDIT_256_SINGLE_OPS: usize = 1_000_000;
const EDIT_256_BULK_BYTES: usize = 64 * 1024 * 1024;
const EDIT_256_INSERT_BYTES: usize = 16 * 1024 * 1024;
const EDIT_256_PER_BYTE_BYTES: usize = 8 * 1024 * 1024;

fn bench_config() -> Config {
    Config {
        page_size: 16 * 1024,
        cache_pages: 128,
        ..Config::default()
    }
}

fn patterned_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

fn shifted_patterned_data(size: usize) -> Vec<u8> {
    (0..size)
        .map(|i| ((i % 256) as u8).wrapping_add(1))
        .collect()
}

fn write_patterned_file(
    name: &str,
    size: usize,
) -> BenchResult<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempdir()?;
    let path = dir.path().join(name);
    let file = fs::File::create(&path)?;
    let mut writer = BufWriter::new(file);
    let chunk = patterned_data((64 * 1024).min(size.max(1)));
    let mut written = 0usize;
    while written < size {
        let take = (size - written).min(chunk.len());
        writer.write_all(&chunk[..take])?;
        written += take;
    }
    writer.flush()?;
    Ok((dir, path))
}

fn write_sparse_zero_file(
    name: &str,
    size: usize,
) -> BenchResult<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempdir()?;
    let path = dir.path().join(name);
    let file = fs::File::create(&path)?;
    file.set_len(size as u64)?;
    Ok((dir, path))
}

fn write_sparse_diff_pair(
    name: &str,
    size: usize,
) -> BenchResult<(tempfile::TempDir, std::path::PathBuf, std::path::PathBuf)> {
    let dir = tempdir()?;
    let current = dir.path().join(format!("{name}-current.bin"));
    let other = dir.path().join(format!("{name}-other.bin"));

    let current_file = fs::File::create(&current)?;
    current_file.set_len(size as u64)?;

    let mut other_file = fs::File::create(&other)?;
    other_file.set_len(size as u64)?;
    other_file.seek(SeekFrom::Start(size as u64 - 1))?;
    other_file.write_all(&[1])?;
    other_file.flush()?;

    Ok((dir, current, other))
}

fn print(label: &str, elapsed: Duration, unit_count: usize) {
    let ns = elapsed.as_nanos();
    let per = ns as f64 / unit_count.max(1) as f64;
    eprintln!("[bench] {label:<48} total {ns:>12} ns  per-op {per:>10.1} ns  (N={unit_count})");
}

#[cfg(unix)]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let max_rss = unsafe { usage.assume_init().ru_maxrss };

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        Some(max_rss as u64)
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        Some((max_rss as u64).saturating_mul(1024))
    }
}

#[cfg(not(unix))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn print_peak_rss(label: &str) {
    match peak_rss_bytes() {
        Some(bytes) => eprintln!("[bench] {label:<48} peak-rss {}", format_bytes(bytes)),
        None => eprintln!("[bench] {label:<48} peak-rss unavailable"),
    }
}

fn make_hasher(algorithm: HashAlgorithm) -> Box<dyn digest::DynDigest> {
    match algorithm {
        HashAlgorithm::Md5 => Box::new(md5::Md5::new()),
        HashAlgorithm::Sha1 => Box::new(sha1::Sha1::new()),
        HashAlgorithm::Sha256 => Box::new(sha2::Sha256::new()),
        HashAlgorithm::Sha512 => Box::new(sha2::Sha512::new()),
        HashAlgorithm::Crc32 => Box::new(Crc32Hasher::new()),
    }
}

struct Crc32Hasher {
    hasher: crc32fast::Hasher,
}

impl Crc32Hasher {
    fn new() -> Self {
        Self {
            hasher: crc32fast::Hasher::new(),
        }
    }
}

impl digest::DynDigest for Crc32Hasher {
    fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    fn finalize_into(self, out: &mut [u8]) -> Result<(), digest::InvalidBufferSize> {
        let checksum = self.hasher.finalize();
        if out.len() < 4 {
            return Err(digest::InvalidBufferSize);
        }
        out[..4].copy_from_slice(&checksum.to_be_bytes());
        Ok(())
    }

    fn finalize_into_reset(&mut self, out: &mut [u8]) -> Result<(), digest::InvalidBufferSize> {
        let checksum = self.hasher.clone().finalize();
        self.hasher = crc32fast::Hasher::new();
        if out.len() < 4 {
            return Err(digest::InvalidBufferSize);
        }
        out[..4].copy_from_slice(&checksum.to_be_bytes());
        Ok(())
    }

    fn reset(&mut self) {
        self.hasher = crc32fast::Hasher::new();
    }

    fn output_size(&self) -> usize {
        4
    }

    fn box_clone(&self) -> Box<dyn digest::DynDigest> {
        Box::new(Self {
            hasher: self.hasher.clone(),
        })
    }
}

fn bench_resolve_piece_heavy() -> BenchResult {
    let (_dir, path) = write_patterned_file("resolve-piece.bin", 4 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..5000u64 {
        let off = i * 800;
        if off < doc.len() {
            doc.insert_bytes(off + (i % 3), &[0xAA, 0xBB])?;
        }
    }

    let len = doc.len();
    let iters = 200_000;
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
    let elapsed = t.elapsed();
    assert_eq!(hit as usize, iters);
    print("resolve random (4MB, ~5k pieces)", elapsed, iters);
    Ok(())
}

fn bench_save_16mb_with_insert() -> BenchResult {
    let (_dir, path) = write_patterned_file("save-insert.bin", 16 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.insert_bytes(doc.len() / 2, &[0xAA, 0xBB])?;

    let t = Instant::now();
    doc.save(None)?;
    print("save 16MB clean+1 insert", t.elapsed(), 1);
    Ok(())
}

fn bench_save_16mb_with_tombstones() -> BenchResult {
    let (_dir, path) = write_patterned_file("save-tombstones.bin", 16 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..4096u64 {
        doc.delete_byte(1_000_000 + i)?;
    }

    let t = Instant::now();
    doc.save(None)?;
    print("save 16MB with 4096 tombstones", t.elapsed(), 1);
    Ok(())
}

fn bench_save_64mb_clean_rewrite() -> BenchResult {
    let (_dir, path) = write_patterned_file("save-64-clean.bin", 64 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    doc.save(None)?;
    print("save 64MB clean rewrite", t.elapsed(), 1);
    Ok(())
}

fn bench_save_64mb_with_middle_insert() -> BenchResult {
    let (_dir, path) = write_patterned_file("save-64-insert.bin", 64 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.insert_bytes(doc.len() / 2, &[0xAA, 0xBB, 0xCC, 0xDD])?;

    let t = Instant::now();
    doc.save(None)?;
    print("save 64MB with middle insert", t.elapsed(), 1);
    Ok(())
}

fn bench_save_64mb_with_tombstone_and_insert() -> BenchResult {
    let (_dir, path) = write_patterned_file("save-64-tombstone-insert.bin", 64 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..8192u64 {
        doc.delete_byte(2_000_000 + i)?;
    }
    doc.insert_bytes(32 * 1024 * 1024, &[0x11, 0x22, 0x33, 0x44])?;

    let t = Instant::now();
    doc.save(None)?;
    print("save 64MB with tombstones+insert", t.elapsed(), 1);
    Ok(())
}

fn bench_save_64mb_overwrite_replacements() -> BenchResult {
    let (_dir, path) = write_patterned_file("save-64-replacements.bin", 64 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..4096u64 {
        doc.replace_display_byte(i * 4096, 0x5A)?;
    }

    let t = Instant::now();
    doc.save(None)?;
    print("save 64MB with sparse replacements", t.elapsed(), 1);
    Ok(())
}

fn bench_parse_elf_format() -> BenchResult {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/elf_header.bin");
    if !path.exists() {
        eprintln!("[bench] parse_elf skipped: no fixture");
        return Ok(());
    }
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let iters = 200;
    for _ in 0..iters {
        let det = format::detect::detect_format(&mut doc);
        if let Some(def) = det {
            let _ = format::parse::parse_format(&def, &mut doc)?;
        }
    }
    print("detect+parse ELF fixture", t.elapsed(), iters);
    Ok(())
}

fn bench_paste_overwrite_large() -> BenchResult {
    let (_dir, path) = write_patterned_file("paste-overwrite.bin", 4 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..2000u64 {
        doc.insert_bytes(i * 500, &[0xAA])?;
    }

    let n = 200_000usize;
    let bytes: Vec<u8> = patterned_data(n);
    let t = Instant::now();
    for (i, &b) in bytes.iter().enumerate() {
        let off = i as u64;
        if doc.cell_id_at(off).is_some() {
            let _ = doc.replace_display_byte(off, b);
        }
    }
    print("paste overwrite 200k bytes into pieced doc", t.elapsed(), n);
    Ok(())
}

fn bench_paste_overwrite_bulk_path() -> BenchResult {
    let (_dir, path) = write_patterned_file("paste-overwrite-bulk.bin", 4 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..2000u64 {
        doc.insert_bytes(i * 500, &[0xAA])?;
    }

    let n = 200_000usize;
    let bytes: Vec<u8> = patterned_data(n);
    let t = Instant::now();
    let ids = doc.cell_ids_range(0, n as u64);
    for (b, id) in bytes.iter().copied().zip(ids.into_iter()) {
        let _ = doc.replace_display_byte_by_id(id, b);
    }
    print("paste overwrite 200k bytes (bulk path)", t.elapsed(), n);
    Ok(())
}

fn bench_edit_mode_replace_nibbles() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-replace-nibbles.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    for i in 0..EDIT_SINGLE_OPS {
        let offset = ((i * 37) % EDIT_FILE_SIZE) as u64;
        doc.replace_nibble(offset, NibblePhase::High, (i as u8) & 0x0f)?;
        doc.replace_nibble(offset, NibblePhase::Low, (i as u8).wrapping_add(1) & 0x0f)?;
    }
    let elapsed = t.elapsed();
    assert!(doc.has_replacements());
    print(
        "edit mode replacement nibbles",
        elapsed,
        EDIT_SINGLE_OPS * 2,
    );
    Ok(())
}

fn bench_edit_mode_insert_nibbles() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-insert-nibbles.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = EDIT_SINGLE_OPS / 2;
    let mut cursor = (EDIT_FILE_SIZE / 2) as u64;

    let t = Instant::now();
    for i in 0..bytes {
        let high = (i as u8) & 0x0f;
        let low = (i as u8).wrapping_add(1) & 0x0f;
        doc.insert_byte(cursor, high << 4)?;
        doc.replace_display_byte(cursor, (high << 4) | low)?;
        cursor += 1;
    }
    let elapsed = t.elapsed();
    assert_eq!(doc.len(), (EDIT_FILE_SIZE + bytes) as u64);
    print("insert mode nibble compose", elapsed, bytes * 2);
    Ok(())
}

fn bench_edit_mode_pending_insert_backspace() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-pending-backspace.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let cursor = (EDIT_FILE_SIZE / 2) as u64;

    let t = Instant::now();
    for i in 0..EDIT_SINGLE_OPS {
        doc.insert_byte(cursor, ((i as u8) & 0x0f) << 4)?;
        let removed = doc.delete_range_real(cursor, 1)?;
        assert_eq!(removed.len(), 1);
    }
    let elapsed = t.elapsed();
    assert_eq!(doc.len(), EDIT_FILE_SIZE as u64);
    print(
        "insert mode pending byte backspace",
        elapsed,
        EDIT_SINGLE_OPS,
    );
    Ok(())
}

fn bench_edit_mode_backspace_real_delete() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-real-delete.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let inserted = patterned_data(EDIT_SINGLE_OPS);
    let offset = (EDIT_FILE_SIZE / 2) as u64;
    doc.insert_bytes(offset, &inserted)?;

    let t = Instant::now();
    for remaining in (1..=EDIT_SINGLE_OPS).rev() {
        let removed = doc.delete_range_real(offset + remaining as u64 - 1, 1)?;
        assert_eq!(removed.len(), 1);
    }
    let elapsed = t.elapsed();
    assert_eq!(doc.len(), EDIT_FILE_SIZE as u64);
    print(
        "insert mode real-delete backspace",
        elapsed,
        EDIT_SINGLE_OPS,
    );
    Ok(())
}

fn bench_edit_mode_normal_tombstone_delete() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-normal-tombstone.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    for i in 0..EDIT_SINGLE_OPS {
        let offset = (i * 32) as u64;
        let id = doc.delete_byte(offset)?;
        assert!(id.is_some());
    }
    let elapsed = t.elapsed();
    assert_eq!(doc.visible_len(), (EDIT_FILE_SIZE - EDIT_SINGLE_OPS) as u64);
    print("normal mode tombstone delete", elapsed, EDIT_SINGLE_OPS);
    Ok(())
}

fn bench_edit_mode_visual_tombstone_range() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-visual-tombstone.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let start = 1024 * 1024_u64;
    let span = EDIT_SINGLE_OPS as u64;

    let t = Instant::now();
    let candidates = doc.cell_ids_range(start, span);
    let mut deleted = 0usize;
    for id in candidates {
        if doc.is_tombstone(id) {
            continue;
        }
        doc.mark_tombstones(&[id])?;
        deleted += 1;
    }
    let elapsed = t.elapsed();
    assert_eq!(deleted, EDIT_SINGLE_OPS);
    print("visual mode tombstone range", elapsed, EDIT_SINGLE_OPS);
    Ok(())
}

fn bench_edit_mode_paste_overwrite_per_byte_1mb() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-paste-overwrite.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = shifted_patterned_data(EDIT_BULK_BYTES);

    let t = Instant::now();
    let (written, changes) =
        doc.overwrite_run_positional(0, bytes.len() as u64, |idx| bytes[idx as usize])?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_BULK_BYTES as u64);
    assert_eq!(changes.len(), EDIT_BULK_BYTES);
    print(
        "paste overwrite 1MB per-byte replacements",
        elapsed,
        EDIT_BULK_BYTES,
    );
    Ok(())
}

fn bench_edit_mode_paste_overwrite_1mb() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-paste-overwrite-compact.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = shifted_patterned_data(EDIT_BULK_BYTES);

    let t = Instant::now();
    let (written, runs) = doc.overwrite_run_bytes_overlay_changed(0, &bytes)?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_BULK_BYTES as u64);
    assert_eq!(runs.len(), 1);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_BULK_BYTES);
    print(
        "paste overwrite 1MB bytes overlay",
        elapsed,
        EDIT_BULK_BYTES,
    );
    Ok(())
}

fn bench_edit_mode_paste_overwrite_16mb() -> BenchResult {
    let (_dir, path) = write_patterned_file(
        "edit-paste-overwrite-compact-16mb.bin",
        EDIT_LARGE_FILE_SIZE,
    )?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = shifted_patterned_data(EDIT_LARGE_BULK_BYTES);

    let t = Instant::now();
    let (written, runs) = doc.overwrite_run_bytes_overlay_changed(0, &bytes)?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_LARGE_BULK_BYTES as u64);
    assert_eq!(runs.len(), 1);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_LARGE_BULK_BYTES);
    print(
        "paste overwrite 16MB bytes overlay",
        elapsed,
        EDIT_LARGE_BULK_BYTES,
    );
    Ok(())
}

fn bench_edit_mode_paste_overwrite_per_byte_4mb() -> BenchResult {
    let (_dir, path) = write_patterned_file(
        "edit-paste-overwrite-per-byte-4mb.bin",
        EDIT_LARGE_FILE_SIZE,
    )?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = shifted_patterned_data(EDIT_PER_BYTE_LARGE_BYTES);

    let t = Instant::now();
    let (written, changes) =
        doc.overwrite_run_positional(0, bytes.len() as u64, |idx| bytes[idx as usize])?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_PER_BYTE_LARGE_BYTES as u64);
    assert_eq!(changes.len(), EDIT_PER_BYTE_LARGE_BYTES);
    print(
        "paste overwrite 4MB per-byte replacements",
        elapsed,
        EDIT_PER_BYTE_LARGE_BYTES,
    );
    Ok(())
}

fn bench_edit_mode_paste_insert_1mb() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-paste-insert.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = patterned_data(EDIT_BULK_BYTES);
    let offset = (EDIT_FILE_SIZE / 2) as u64;

    let t = Instant::now();
    let inserted = doc.insert_bytes(offset, &bytes)?;
    let elapsed = t.elapsed();
    assert_eq!(inserted.len(), EDIT_BULK_BYTES);
    assert_eq!(doc.len(), (EDIT_FILE_SIZE + EDIT_BULK_BYTES) as u64);
    print("paste insert 1MB real insert", elapsed, EDIT_BULK_BYTES);
    Ok(())
}

fn bench_edit_mode_fill_overlay_1mb() -> BenchResult {
    let (_dir, path) = write_patterned_file("edit-fill-overlay.bin", EDIT_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let pattern = [0xde_u8, 0xad, 0xbe, 0xef];

    let t = Instant::now();
    let stats = doc.overwrite_run_pattern_overlay(0, EDIT_BULK_BYTES as u64, &pattern)?;
    let elapsed = t.elapsed();
    assert_eq!(stats.visited, EDIT_BULK_BYTES as u64);
    assert_eq!(stats.changed, EDIT_BULK_BYTES as u64);
    print("fill command 1MB range overlay", elapsed, EDIT_BULK_BYTES);
    Ok(())
}

fn bench_edit_256mb_replace_nibbles() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-replace-nibbles.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    for i in 0..EDIT_256_SINGLE_OPS {
        let offset = (i * 257) as u64;
        doc.replace_nibble(offset, NibblePhase::High, (i as u8) & 0x0f)?;
        doc.replace_nibble(offset, NibblePhase::Low, (i as u8).wrapping_add(1) & 0x0f)?;
    }
    let elapsed = t.elapsed();
    assert!(doc.has_replacements());
    print(
        "edit 256MB replacement nibbles",
        elapsed,
        EDIT_256_SINGLE_OPS * 2,
    );
    Ok(())
}

fn bench_edit_256mb_insert_nibbles() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-insert-nibbles.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let mut cursor = (EDIT_256_FILE_SIZE / 2) as u64;

    let t = Instant::now();
    for i in 0..EDIT_256_SINGLE_OPS {
        let high = (i as u8) & 0x0f;
        let low = (i as u8).wrapping_add(1) & 0x0f;
        doc.insert_byte(cursor, high << 4)?;
        doc.replace_display_byte(cursor, (high << 4) | low)?;
        cursor += 1;
    }
    let elapsed = t.elapsed();
    assert_eq!(doc.len(), (EDIT_256_FILE_SIZE + EDIT_256_SINGLE_OPS) as u64);
    print(
        "edit 256MB insert nibble compose",
        elapsed,
        EDIT_256_SINGLE_OPS * 2,
    );
    Ok(())
}

fn bench_edit_256mb_pending_insert_backspace() -> BenchResult {
    let (_dir, path) =
        write_sparse_zero_file("edit-256-pending-backspace.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let cursor = (EDIT_256_FILE_SIZE / 2) as u64;

    let t = Instant::now();
    for i in 0..EDIT_256_SINGLE_OPS {
        doc.insert_byte(cursor, ((i as u8) & 0x0f) << 4)?;
        let removed = doc.delete_range_real(cursor, 1)?;
        assert_eq!(removed.len(), 1);
    }
    let elapsed = t.elapsed();
    assert_eq!(doc.len(), EDIT_256_FILE_SIZE as u64);
    print(
        "edit 256MB pending byte backspace",
        elapsed,
        EDIT_256_SINGLE_OPS,
    );
    Ok(())
}

fn bench_edit_256mb_backspace_real_delete() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-real-delete.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let offset = (EDIT_256_FILE_SIZE / 2) as u64;
    let inserted = vec![0x5a; EDIT_256_SINGLE_OPS];
    doc.insert_bytes(offset, &inserted)?;

    let t = Instant::now();
    for remaining in (1..=EDIT_256_SINGLE_OPS).rev() {
        let removed = doc.delete_range_real(offset + remaining as u64 - 1, 1)?;
        assert_eq!(removed.len(), 1);
    }
    let elapsed = t.elapsed();
    assert_eq!(doc.len(), EDIT_256_FILE_SIZE as u64);
    print(
        "edit 256MB real-delete backspace",
        elapsed,
        EDIT_256_SINGLE_OPS,
    );
    Ok(())
}

fn bench_edit_256mb_normal_tombstone_delete() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-normal-tombstone.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    for i in 0..EDIT_256_SINGLE_OPS {
        let offset = (i * 257) as u64;
        let id = doc.delete_byte(offset)?;
        assert!(id.is_some());
    }
    let elapsed = t.elapsed();
    assert_eq!(
        doc.visible_len(),
        (EDIT_256_FILE_SIZE - EDIT_256_SINGLE_OPS) as u64
    );
    print(
        "edit 256MB normal tombstone delete",
        elapsed,
        EDIT_256_SINGLE_OPS,
    );
    Ok(())
}

fn bench_edit_256mb_visual_tombstone_range() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-visual-tombstone.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let start = 64 * 1024 * 1024_u64;
    let span = EDIT_256_SINGLE_OPS as u64;

    let t = Instant::now();
    let candidates = doc.cell_ids_range(start, span);
    let mut deleted = 0usize;
    for id in candidates {
        if doc.is_tombstone(id) {
            continue;
        }
        doc.mark_tombstones(&[id])?;
        deleted += 1;
    }
    let elapsed = t.elapsed();
    assert_eq!(deleted, EDIT_256_SINGLE_OPS);
    print(
        "edit 256MB visual tombstone range",
        elapsed,
        EDIT_256_SINGLE_OPS,
    );
    Ok(())
}

fn bench_edit_256mb_paste_overwrite_overlay_64mb() -> BenchResult {
    let (_dir, path) =
        write_sparse_zero_file("edit-256-paste-overwrite-overlay.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = vec![1_u8; EDIT_256_BULK_BYTES];

    let t = Instant::now();
    let (written, runs) = doc.overwrite_run_bytes_overlay_changed(0, &bytes)?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_256_BULK_BYTES as u64);
    assert_eq!(runs.len(), 1);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);
    print(
        "edit 256MB paste overwrite 64MB overlay",
        elapsed,
        EDIT_256_BULK_BYTES,
    );
    Ok(())
}

fn bench_edit_256mb_paste_overwrite_per_byte_8mb() -> BenchResult {
    let (_dir, path) =
        write_sparse_zero_file("edit-256-paste-overwrite-per-byte.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = vec![1_u8; EDIT_256_PER_BYTE_BYTES];

    let t = Instant::now();
    let (written, changes) =
        doc.overwrite_run_positional(0, bytes.len() as u64, |idx| bytes[idx as usize])?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_256_PER_BYTE_BYTES as u64);
    assert_eq!(changes.len(), EDIT_256_PER_BYTE_BYTES);
    print(
        "edit 256MB paste overwrite 8MB per-byte",
        elapsed,
        EDIT_256_PER_BYTE_BYTES,
    );
    Ok(())
}

fn bench_edit_256mb_paste_insert_16mb() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-paste-insert.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = vec![0x5a; EDIT_256_INSERT_BYTES];
    let offset = (EDIT_256_FILE_SIZE / 2) as u64;

    let t = Instant::now();
    let inserted = doc.insert_bytes(offset, &bytes)?;
    let elapsed = t.elapsed();
    assert_eq!(inserted.len(), EDIT_256_INSERT_BYTES);
    assert_eq!(
        doc.len(),
        (EDIT_256_FILE_SIZE + EDIT_256_INSERT_BYTES) as u64
    );
    print(
        "edit 256MB paste insert 16MB",
        elapsed,
        EDIT_256_INSERT_BYTES,
    );
    Ok(())
}

fn bench_edit_256mb_fill_overlay_64mb() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-fill-overlay.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let pattern = [0x7f_u8];

    let t = Instant::now();
    let stats = doc.overwrite_run_pattern_overlay(0, EDIT_256_BULK_BYTES as u64, &pattern)?;
    let elapsed = t.elapsed();
    assert_eq!(stats.visited, EDIT_256_BULK_BYTES as u64);
    assert_eq!(stats.changed, EDIT_256_BULK_BYTES as u64);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);
    print("edit 256MB fill 64MB overlay", elapsed, EDIT_256_BULK_BYTES);
    Ok(())
}

fn bench_logical_bytes_large_copy() -> BenchResult {
    let (_dir, path) = write_patterned_file("logical-bytes.bin", 8 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.delete_byte(100)?;

    let t = Instant::now();
    let bytes = doc.logical_bytes(0, doc.len() - 1)?;
    let elapsed = t.elapsed();
    assert!(!bytes.is_empty());
    print("logical_bytes 8MB copy", elapsed, 1);
    Ok(())
}

fn bench_export_stream_64mb() -> BenchResult {
    let (_dir, path) = write_patterned_file("export-stream.bin", 64 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let out_dir = tempdir()?;
    let out_path = out_dir.path().join("export-out.bin");

    let t = Instant::now();
    let file = fs::File::create(&out_path)?;
    let mut writer = std::io::BufWriter::new(file);
    let written = doc.for_each_logical_chunk(0, doc.len() - 1, |chunk| {
        writer
            .write_all(chunk)
            .map_err(hxedit::error::HxError::from)
    })?;
    writer.flush()?;
    let elapsed = t.elapsed();
    assert_eq!(written, 64 * 1024 * 1024);
    print("export stream 64MB to file", elapsed, 1);
    Ok(())
}

fn bench_fill_stream_4mb() -> BenchResult {
    // Smaller than the export bench: fill writes one replacement-map entry per
    // byte, so the BTreeMap cost (existing replacement semantics) dominates and
    // would make a 64 MB run take many seconds. 4 MB still spans dozens of
    // 64 KB chunks, exercising the streaming path.
    let (_dir, path) = write_patterned_file("fill-stream.bin", 4 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let pattern = [0xde_u8, 0xad, 0xbe];

    let t = Instant::now();
    let (written, _changes) = doc.overwrite_run_positional(0, doc.len(), |run_index| {
        pattern[(run_index % pattern.len() as u64) as usize]
    })?;
    let elapsed = t.elapsed();
    assert_eq!(written, 4 * 1024 * 1024);
    print("fill stream 4MB repeating pattern", elapsed, 1);
    Ok(())
}

fn bench_xor_stream_4mb() -> BenchResult {
    // See `bench_fill_stream_4mb`: in-place xor also writes per-byte
    // replacements, so keep the span at 4 MB to stay chunk-representative
    // without a multi-second BTreeMap write.
    let (_dir, path) = write_patterned_file("xor-stream.bin", 4 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let (visited, _changes) =
        doc.transform_visible_range_in_place(0, doc.len() - 1, |byte| byte ^ 0x5a)?;
    let elapsed = t.elapsed();
    assert_eq!(visited, 4 * 1024 * 1024);
    print("xor! stream 4MB in place", elapsed, 1);
    Ok(())
}

fn bench_hash_sha256_16mb() -> BenchResult {
    let (_dir, path) = write_patterned_file("hash-sha256.bin", 16 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let (bytes_hashed, hash_bytes) =
        doc.hash_logical_bytes(0, doc.len() - 1, make_hasher(HashAlgorithm::Sha256))?;
    let elapsed = t.elapsed();
    assert_eq!(bytes_hashed, 16 * 1024 * 1024);
    assert_eq!(hash_bytes.len(), 32);
    print("hash 16MB sha256", elapsed, 1);
    Ok(())
}

fn bench_hash_crc32_16mb() -> BenchResult {
    let (_dir, path) = write_patterned_file("hash-crc32.bin", 16 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let (bytes_hashed, hash_bytes) =
        doc.hash_logical_bytes(0, doc.len() - 1, make_hasher(HashAlgorithm::Crc32))?;
    let elapsed = t.elapsed();
    assert_eq!(bytes_hashed, 16 * 1024 * 1024);
    assert_eq!(hash_bytes.len(), 4);
    print("hash 16MB crc32", elapsed, 1);
    Ok(())
}

fn bench_hash_16mb_with_tombstones() -> BenchResult {
    let (_dir, path) = write_patterned_file("hash-tombstones.bin", 16 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.delete_byte(100)?;
    doc.delete_byte(1_000_000)?;

    let t = Instant::now();
    let (bytes_hashed, _) =
        doc.hash_logical_bytes(0, doc.len() - 1, make_hasher(HashAlgorithm::Sha256))?;
    let elapsed = t.elapsed();
    assert_eq!(bytes_hashed, 16 * 1024 * 1024 - 2);
    print("hash 16MB sha256 with 2 tombstones", elapsed, 1);
    Ok(())
}

fn bench_hash_16mb_with_insert() -> BenchResult {
    let (_dir, path) = write_patterned_file("hash-insert.bin", 16 * 1024 * 1024)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.insert_bytes(doc.len() / 2, &[0xAA, 0xBB])?;

    let t = Instant::now();
    let (bytes_hashed, _) =
        doc.hash_logical_bytes(0, doc.len() - 1, make_hasher(HashAlgorithm::Md5))?;
    let elapsed = t.elapsed();
    assert_eq!(bytes_hashed, 16 * 1024 * 1024 + 2);
    print("hash 16MB md5 with insert", elapsed, 1);
    Ok(())
}

fn bench_search_16mb_file() -> BenchResult {
    let dir = tempdir()?;
    let path = dir.path().join("search-current.bin");
    let size: usize = 16 * 1024 * 1024;
    let mut data = vec![0u8; size];
    let forward_needle = [0xde, 0xad, 0xbe, 0xef];
    let backward_needle = [0xca, 0xfe, 0xba, 0xbe];
    let forward_offset = size - forward_needle.len();
    let backward_offset = 128usize;
    data[forward_offset..forward_offset + forward_needle.len()].copy_from_slice(&forward_needle);
    data[backward_offset..backward_offset + backward_needle.len()]
        .copy_from_slice(&backward_needle);
    fs::write(&path, &data)?;

    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let forward = doc.search_forward(0, &forward_needle)?;
    let forward_elapsed = t.elapsed();
    assert_eq!(forward, Some(forward_offset as u64));
    print("search 16MB forward miss-until-tail", forward_elapsed, 1);

    let t = Instant::now();
    let backward = doc.search_backward(doc.len(), &backward_needle)?;
    let backward_elapsed = t.elapsed();
    assert_eq!(backward, Some(backward_offset as u64));
    print("search 16MB backward miss-until-head", backward_elapsed, 1);
    Ok(())
}

fn bench_search_256mb_clean_memmem() -> BenchResult {
    // Clean document => SIMD memmem path. Worst case: needle at the very tail,
    // forcing a full scan. This is the headline win over the old byte-at-a-time
    // KMP loop (~24x on the matching cost at this size).
    let dir = tempdir()?;
    let path = dir.path().join("search-256-clean.bin");
    let size: usize = 256 * 1024 * 1024;
    let mut data = vec![0u8; size];
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let offset = size - needle.len();
    data[offset..].copy_from_slice(&needle);
    fs::write(&path, &data)?;

    let mut doc = Document::open(&path, &bench_config())?;
    let t = Instant::now();
    let found = doc.search_forward(0, &needle)?;
    let elapsed = t.elapsed();
    assert_eq!(found, Some(offset as u64));
    print("search 256MB clean forward (memmem)", elapsed, 1);
    Ok(())
}

fn bench_search_256mb_dirty_one_tombstone() -> BenchResult {
    // A single tombstone marks the document dirty, so the search takes the
    // per-piece path. With option B the large clean chunks still run memmem
    // (only the chunk holding the tombstone falls back to byte-at-a-time), so
    // this should stay close to the clean 256MB number (~50ms) rather than the
    // old whole-document KMP (~330ms).
    let dir = tempdir()?;
    let path = dir.path().join("search-256-dirty.bin");
    let size: usize = 256 * 1024 * 1024;
    let mut data = vec![0u8; size];
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let offset = size - needle.len();
    data[offset..].copy_from_slice(&needle);
    fs::write(&path, &data)?;

    let mut doc = Document::open(&path, &bench_config())?;
    doc.delete_byte(5)?; // tombstone near the start -> dirty path

    let t = Instant::now();
    let found = doc.search_forward(0, &needle)?;
    let elapsed = t.elapsed();
    assert_eq!(found, Some(offset as u64)); // tombstones keep their display slot
    print("search 256MB dirty(1 tombstone) forward", elapsed, 1);
    Ok(())
}

fn bench_diff_next_tail_mismatch_64mb() -> BenchResult {
    bench_diff_next_tail_mismatch("diff-next-64", 64 * 1024 * 1024)
}

fn bench_diff_next_tail_mismatch_256mb() -> BenchResult {
    bench_diff_next_tail_mismatch("diff-next-256", 256 * 1024 * 1024)
}

fn bench_diff_next_tail_mismatch_256mb_stepper() -> BenchResult {
    let size = 256 * 1024 * 1024;
    let (_dir, current, other) = write_sparse_diff_pair("diff-next-256-stepper", size)?;
    let config = bench_config();
    let mut document = Document::open(&current, &config)?;
    let mut other_view = FileView::open(&other, true, config.page_size, config.cache_pages)?;
    let other_len = other_view.len();

    let mut cursor = 1_u64;
    let end = size as u64 - 1;
    let mut steps = 0_usize;
    let t = Instant::now();
    let found = loop {
        let step = find_mismatch_forward_step(
            &mut document,
            &mut other_view,
            other_len,
            cursor,
            end,
            128 * 1024 * 1024,
        )?;
        steps += 1;
        if let Some(found) = step.found {
            break Some(found);
        }
        let Some(next) = step.next else {
            break None;
        };
        cursor = next;
    };
    let elapsed = t.elapsed();

    assert_eq!(found, Some(end));
    print("diff next tail mismatch 256MB stepper", elapsed, steps);
    Ok(())
}

fn bench_diff_next_tail_mismatch(name: &str, size: usize) -> BenchResult {
    let (_dir, current, other) = write_sparse_diff_pair(name, size)?;
    let config = bench_config();
    let mut document = Document::open(&current, &config)?;
    let mut other_view = FileView::open(&other, true, config.page_size, config.cache_pages)?;
    let other_len = other_view.len();

    let t = Instant::now();
    let found = find_mismatch_forward(&mut document, &mut other_view, other_len, 1)?;
    let elapsed = t.elapsed();

    assert_eq!(found, Some(size as u64 - 1));
    print(
        &format!("diff next tail mismatch {}MB", size / 1024 / 1024),
        elapsed,
        1,
    );
    Ok(())
}

fn run_in_process(label: &str, bench: fn() -> BenchResult) -> bool {
    eprintln!("[bench] running {label}");
    let success = match bench() {
        Ok(()) => true,
        Err(err) => {
            eprintln!("[bench] {label} failed: {err}");
            false
        }
    };
    print_peak_rss(label);
    success
}

fn run_isolated(label: &str) -> bool {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            eprintln!("[bench] {label} failed: unable to resolve current executable: {err}");
            return false;
        }
    };

    match Command::new(exe)
        .env(BENCH_CHILD_ENV, label)
        .env("HXEDIT_RUN_BENCH", "1")
        .status()
    {
        Ok(status) => status.success(),
        Err(err) => {
            eprintln!("[bench] {label} failed: unable to spawn isolated child: {err}");
            false
        }
    }
}

fn benches() -> &'static [BenchEntry] {
    &[
        ("resolve_piece_heavy", bench_resolve_piece_heavy),
        ("save_16mb_with_insert", bench_save_16mb_with_insert),
        ("save_16mb_with_tombstones", bench_save_16mb_with_tombstones),
        ("save_64mb_clean_rewrite", bench_save_64mb_clean_rewrite),
        (
            "save_64mb_with_middle_insert",
            bench_save_64mb_with_middle_insert,
        ),
        (
            "save_64mb_with_tombstone_and_insert",
            bench_save_64mb_with_tombstone_and_insert,
        ),
        (
            "save_64mb_overwrite_replacements",
            bench_save_64mb_overwrite_replacements,
        ),
        ("parse_elf_format", bench_parse_elf_format),
        ("paste_overwrite_large", bench_paste_overwrite_large),
        ("paste_overwrite_bulk_path", bench_paste_overwrite_bulk_path),
        ("edit_mode_replace_nibbles", bench_edit_mode_replace_nibbles),
        ("edit_mode_insert_nibbles", bench_edit_mode_insert_nibbles),
        (
            "edit_mode_pending_insert_backspace",
            bench_edit_mode_pending_insert_backspace,
        ),
        (
            "edit_mode_backspace_real_delete",
            bench_edit_mode_backspace_real_delete,
        ),
        (
            "edit_mode_normal_tombstone_delete",
            bench_edit_mode_normal_tombstone_delete,
        ),
        (
            "edit_mode_visual_tombstone_range",
            bench_edit_mode_visual_tombstone_range,
        ),
        (
            "edit_mode_paste_overwrite_1mb",
            bench_edit_mode_paste_overwrite_1mb,
        ),
        (
            "edit_mode_paste_overwrite_per_byte_1mb",
            bench_edit_mode_paste_overwrite_per_byte_1mb,
        ),
        (
            "edit_mode_paste_overwrite_16mb",
            bench_edit_mode_paste_overwrite_16mb,
        ),
        (
            "edit_mode_paste_overwrite_per_byte_4mb",
            bench_edit_mode_paste_overwrite_per_byte_4mb,
        ),
        (
            "edit_mode_paste_insert_1mb",
            bench_edit_mode_paste_insert_1mb,
        ),
        (
            "edit_mode_fill_overlay_1mb",
            bench_edit_mode_fill_overlay_1mb,
        ),
        (
            "edit_256mb_replace_nibbles",
            bench_edit_256mb_replace_nibbles,
        ),
        ("edit_256mb_insert_nibbles", bench_edit_256mb_insert_nibbles),
        (
            "edit_256mb_pending_insert_backspace",
            bench_edit_256mb_pending_insert_backspace,
        ),
        (
            "edit_256mb_backspace_real_delete",
            bench_edit_256mb_backspace_real_delete,
        ),
        (
            "edit_256mb_normal_tombstone_delete",
            bench_edit_256mb_normal_tombstone_delete,
        ),
        (
            "edit_256mb_visual_tombstone_range",
            bench_edit_256mb_visual_tombstone_range,
        ),
        (
            "edit_256mb_paste_overwrite_overlay_64mb",
            bench_edit_256mb_paste_overwrite_overlay_64mb,
        ),
        (
            "edit_256mb_paste_overwrite_per_byte_8mb",
            bench_edit_256mb_paste_overwrite_per_byte_8mb,
        ),
        (
            "edit_256mb_paste_insert_16mb",
            bench_edit_256mb_paste_insert_16mb,
        ),
        (
            "edit_256mb_fill_overlay_64mb",
            bench_edit_256mb_fill_overlay_64mb,
        ),
        ("logical_bytes_large_copy", bench_logical_bytes_large_copy),
        ("export_stream_64mb", bench_export_stream_64mb),
        ("fill_stream_4mb", bench_fill_stream_4mb),
        ("xor_stream_4mb", bench_xor_stream_4mb),
        ("hash_sha256_16mb", bench_hash_sha256_16mb),
        ("hash_crc32_16mb", bench_hash_crc32_16mb),
        ("hash_16mb_with_tombstones", bench_hash_16mb_with_tombstones),
        ("hash_16mb_with_insert", bench_hash_16mb_with_insert),
        ("search_16mb_file", bench_search_16mb_file),
        ("search_256mb_clean_memmem", bench_search_256mb_clean_memmem),
        (
            "search_256mb_dirty_one_tombstone",
            bench_search_256mb_dirty_one_tombstone,
        ),
        (
            "diff_next_tail_mismatch_64mb",
            bench_diff_next_tail_mismatch_64mb,
        ),
        (
            "diff_next_tail_mismatch_256mb",
            bench_diff_next_tail_mismatch_256mb,
        ),
        (
            "diff_next_tail_mismatch_256mb_stepper",
            bench_diff_next_tail_mismatch_256mb_stepper,
        ),
    ]
}

fn main() {
    let benches = benches();
    if let Some(child_label) = std::env::var_os(BENCH_CHILD_ENV) {
        let child_label = child_label.to_string_lossy();
        let Some((label, bench)) = benches
            .iter()
            .find(|(label, _)| *label == child_label.as_ref())
        else {
            eprintln!("[bench] unknown child bench {child_label}");
            std::process::exit(1);
        };
        if !run_in_process(label, *bench) {
            std::process::exit(1);
        }
        return;
    }

    if cfg!(debug_assertions) && std::env::var_os("HXEDIT_RUN_BENCH").is_none() {
        eprintln!(
            "[bench] skipped in debug/test profile; run `cargo bench --bench perf_bench` for timings"
        );
        return;
    }

    let filter = std::env::var("HXEDIT_BENCH_FILTER").ok();
    let failed = benches
        .iter()
        .filter(|(label, _)| {
            filter
                .as_deref()
                .is_none_or(|needle| label.contains(needle))
        })
        .filter(|(label, _)| !run_isolated(label))
        .count();
    if failed > 0 {
        std::process::exit(1);
    }
}
