//! Performance benchmark harness.
//!
//! Run with: `cargo bench --bench perf_bench`

use std::fs;
#[cfg(feature = "disasm-iced-x86")]
use std::hint::black_box;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use digest::Digest;
use hxedit::commands::types::HashAlgorithm;
use hxedit::config::Config;
use hxedit::core::document::Document;
use hxedit::core::file_view::FileView;
use hxedit::diff::{find_mismatch_forward, find_mismatch_forward_step};
use hxedit::format;
use hxedit::mode::NibblePhase;
#[cfg(feature = "disasm-iced-x86")]
use ratatui::buffer::Buffer;
#[cfg(feature = "disasm-iced-x86")]
use ratatui::layout::Rect;
#[cfg(feature = "disasm-iced-x86")]
use ratatui::widgets::{Paragraph, Widget};
use tempfile::tempdir;

type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type BenchFn = fn() -> BenchResult;
type BenchEntry = (&'static str, BenchFn);

const BENCH_CHILD_ENV: &str = "HXEDIT_BENCH_CHILD";
const BENCH_REPEAT_ENV: &str = "HXEDIT_BENCH_REPEAT";
const BENCH_SUITE_ENV: &str = "HXEDIT_BENCH_SUITE";
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
const LARGE_FILE_SIZE_1GIB: usize = 1024 * 1024 * 1024;
const PATTERNED_FILE_SIZE_256MB: usize = 256 * 1024 * 1024;

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

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_usize(&mut self, upper_exclusive: usize) -> usize {
        if upper_exclusive == 0 {
            return 0;
        }
        (self.next_u64() as usize) % upper_exclusive
    }

    fn next_byte(&mut self) -> u8 {
        (self.next_u64() >> 32) as u8
    }
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

fn write_sparse_file_with_tail_needle(
    name: &str,
    size: usize,
    needle: &[u8],
) -> BenchResult<(tempfile::TempDir, std::path::PathBuf, usize)> {
    let (dir, path) = write_sparse_zero_file(name, size)?;
    let offset = size - needle.len();
    let mut file = fs::OpenOptions::new().write(true).open(&path)?;
    file.seek(SeekFrom::Start(offset as u64))?;
    file.write_all(needle)?;
    file.flush()?;
    Ok((dir, path, offset))
}

fn print(label: &str, elapsed: Duration, unit_count: usize) {
    let ms = elapsed.as_secs_f64() * 1000.0;
    let per = elapsed.as_nanos() as f64 / unit_count.max(1) as f64;
    eprintln!("[bench] {label:<48} total {ms:>12.6} ms  per-op {per:>10.1} ns  (N={unit_count})");
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

#[derive(Debug, Clone, Copy)]
struct BenchMeasurement {
    elapsed_ms: f64,
    peak_rss: Option<u64>,
}

fn display_range_has_tombstone(doc: &Document, offset: u64, len: u64) -> bool {
    if len == 0 || offset >= doc.len() || !doc.has_tombstones() {
        return false;
    }
    let mut cursor = offset;
    let end = offset.saturating_add(len).min(doc.len());
    while cursor < end {
        let batch = (end - cursor).min(64 * 1024);
        if doc
            .cell_ids_range(cursor, batch)
            .into_iter()
            .any(|id| doc.is_tombstone(id))
        {
            return true;
        }
        cursor += batch;
    }
    false
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

fn bench_save_256mb_patterned_clean_rewrite() -> BenchResult {
    let (_dir, path) =
        write_patterned_file("save-256-patterned-clean.bin", PATTERNED_FILE_SIZE_256MB)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    doc.save(None)?;
    print("save 256MB patterned clean rewrite", t.elapsed(), 1);
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

fn bench_save_1gib_with_middle_insert() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("save-1gib-insert.bin", LARGE_FILE_SIZE_1GIB)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.insert_bytes(doc.len() / 2, &[0xAA, 0xBB, 0xCC, 0xDD])?;

    let t = Instant::now();
    doc.save(None)?;
    print("save 1GiB with middle insert", t.elapsed(), 1);
    Ok(())
}

fn bench_save_1gib_with_tombstone_and_insert() -> BenchResult {
    let (_dir, path) =
        write_sparse_zero_file("save-1gib-tombstone-insert.bin", LARGE_FILE_SIZE_1GIB)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..65_536u64 {
        doc.delete_byte(16 * 1024 * 1024 + i * 1024)?;
    }
    doc.insert_bytes(doc.len() / 2, &[0x11, 0x22, 0x33, 0x44])?;

    let t = Instant::now();
    doc.save(None)?;
    print("save 1GiB with 65536 tombstones+insert", t.elapsed(), 1);
    Ok(())
}

fn bench_save_1gib_with_64mb_range_overlay() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("save-1gib-range-overlay.bin", LARGE_FILE_SIZE_1GIB)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let stats = doc.overwrite_run_pattern_overlay(
        512 * 1024 * 1024,
        EDIT_256_BULK_BYTES as u64,
        &[0x11, 0x22, 0x33, 0x44],
    )?;
    assert_eq!(stats.visited, EDIT_256_BULK_BYTES as u64);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);

    let t = Instant::now();
    doc.save(None)?;
    print("save 1GiB with 64MB range overlay", t.elapsed(), 1);
    Ok(())
}

fn bench_save_1gib_with_sparse_replacement_islands() -> BenchResult {
    let (_dir, path) =
        write_sparse_zero_file("save-1gib-sparse-islands.bin", LARGE_FILE_SIZE_1GIB)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let islands = 65_536u64;
    for i in 0..islands {
        let value = ((i % 255) + 1) as u8;
        doc.replace_display_byte(i * 16 * 1024, value)?;
    }
    assert_eq!(doc.replacement_dirty_bytes(), islands as usize);

    let t = Instant::now();
    doc.save(None)?;
    print(
        "save 1GiB with 65536 sparse replacement islands",
        t.elapsed(),
        1,
    );
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

#[cfg(feature = "disasm-iced-x86")]
fn x86_64_elf_bytes(code: &[u8]) -> Vec<u8> {
    let text_virtual = 0x401000u64;
    let mut bytes = vec![0_u8; (0x100 + code.len()).max(0x200)];
    bytes[0..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x100u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    let ph = 64usize;
    bytes[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[ph + 4..ph + 8].copy_from_slice(&0x5u32.to_le_bytes());
    bytes[ph + 8..ph + 16].copy_from_slice(&0x100u64.to_le_bytes());
    bytes[ph + 16..ph + 24].copy_from_slice(&text_virtual.to_le_bytes());
    bytes[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
    bytes[0x100..0x100 + code.len()].copy_from_slice(code);
    bytes
}

#[cfg(feature = "disasm-iced-x86")]
fn bench_disasm_decode_x86_64_nop_rows() -> BenchResult {
    let dir = tempdir()?;
    let path = dir.path().join("disasm-decode-x86_64-nop.bin");
    let code = vec![0x90_u8; 128 * 1024];
    fs::write(&path, x86_64_elf_bytes(&code))?;

    let mut doc = Document::open(&path, &bench_config())?;
    let info = hxedit::executable::detect_executable_info(&mut doc)
        .ok_or_else(|| std::io::Error::other("synthetic ELF was not detected"))?;
    let backend = hxedit::disasm::backend::resolve_backend(&info, None)?;

    let t = Instant::now();
    let rows =
        hxedit::disasm::decode_region_rows(&mut doc, &info, backend.as_ref(), 0x100, code.len())?;
    let elapsed = t.elapsed();
    assert_eq!(rows.len(), code.len());
    assert!(rows.iter().all(|row| row.text == "nop"));
    print("disasm decode x86_64 nop rows", elapsed, rows.len());
    Ok(())
}

#[cfg(not(feature = "disasm-iced-x86"))]
fn bench_disasm_decode_x86_64_nop_rows() -> BenchResult {
    eprintln!("[bench] disasm decode x86_64 skipped: disasm-iced-x86 feature disabled");
    Ok(())
}

#[cfg(feature = "disasm-iced-x86")]
fn bench_disasm_render_x86_64_nop_frames() -> BenchResult {
    bench_disasm_render_x86_64_frames("nop", &[0x90], false)
}

#[cfg(not(feature = "disasm-iced-x86"))]
fn bench_disasm_render_x86_64_nop_frames() -> BenchResult {
    eprintln!("[bench] disasm render x86_64 nop skipped: disasm-iced-x86 feature disabled");
    Ok(())
}

#[cfg(feature = "disasm-iced-x86")]
fn bench_disasm_render_x86_64_nop_rail_frames() -> BenchResult {
    bench_disasm_render_x86_64_frames("nop-rail", &[0x90], true)
}

#[cfg(not(feature = "disasm-iced-x86"))]
fn bench_disasm_render_x86_64_nop_rail_frames() -> BenchResult {
    eprintln!("[bench] disasm render x86_64 nop rail skipped: disasm-iced-x86 feature disabled");
    Ok(())
}

#[cfg(feature = "disasm-iced-x86")]
fn bench_disasm_render_x86_64_jmp_frames() -> BenchResult {
    bench_disasm_render_x86_64_frames("jmp", &[0xeb, 0x00], true)
}

#[cfg(not(feature = "disasm-iced-x86"))]
fn bench_disasm_render_x86_64_jmp_frames() -> BenchResult {
    eprintln!("[bench] disasm render x86_64 jmp skipped: disasm-iced-x86 feature disabled");
    Ok(())
}

#[cfg(feature = "disasm-iced-x86")]
fn bench_disasm_render_x86_64_frames(
    name: &str,
    instruction: &[u8],
    jump_rail: bool,
) -> BenchResult {
    let dir = tempdir()?;
    let path = dir.path().join(format!("disasm-render-x86_64-{name}.bin"));
    let row_count = 80usize;
    let frames = 5_000usize;
    let mut code = Vec::with_capacity(256 * 1024);
    while code.len() < 256 * 1024 {
        code.extend_from_slice(instruction);
    }
    fs::write(&path, x86_64_elf_bytes(&code))?;

    let mut doc = Document::open(&path, &bench_config())?;
    let info = hxedit::executable::detect_executable_info(&mut doc)
        .ok_or_else(|| std::io::Error::other("synthetic ELF was not detected"))?;
    let backend = hxedit::disasm::backend::resolve_backend(&info, None)?;
    let mut cache = hxedit::disasm::DisasmCache::new(&info, doc.len());
    let palette = hxedit::view::palette::Palette::new(hxedit::view::palette::ColorLevel::Basic);
    let area = Rect::new(0, 0, 160, row_count as u16);
    let gutter_area = Rect::new(0, 0, 18, row_count as u16);
    let bytes_area = Rect::new(19, 0, 24, row_count as u16);
    let text_area = Rect::new(44, 0, 110, row_count as u16);

    let t = Instant::now();
    let mut collect_elapsed = Duration::default();
    let mut line_elapsed = Duration::default();
    let mut rail_elapsed = Duration::default();
    let mut draw_elapsed = Duration::default();
    let mut rendered_lines = 0usize;
    for _ in 0..frames {
        let section = Instant::now();
        let rows = cache.collect_rows(&mut doc, &info, backend.as_ref(), 0x100, row_count)?;
        collect_elapsed += section.elapsed();

        let section = Instant::now();
        let display =
            hxedit::view::disasm_grid::build_display(&rows, 18, 0x100, None, 80, &palette);
        line_elapsed += section.elapsed();

        let text = if jump_rail {
            let section = Instant::now();
            let rail = hxedit::view::disasm_grid::build_jump_rail(
                &rows,
                &display.row_sources,
                &display.text,
                110,
                &palette,
            );
            let text = hxedit::view::disasm_grid::merge_jump_rail(display.text, &rail, &palette);
            rail_elapsed += section.elapsed();
            text
        } else {
            display.text
        };
        rendered_lines += text.len();

        let section = Instant::now();
        let mut buffer = Buffer::empty(area);
        Paragraph::new(display.gutter).render(gutter_area, &mut buffer);
        Paragraph::new(display.bytes).render(bytes_area, &mut buffer);
        Paragraph::new(text).render(text_area, &mut buffer);
        black_box(&buffer);
        draw_elapsed += section.elapsed();
    }
    let elapsed = t.elapsed();
    assert!(rendered_lines >= frames * row_count);
    eprintln!(
        "[bench] disasm render x86_64 {name} breakdown          collect {:>10.3} us/frame  lines {:>10.3} us/frame  rail {:>10.3} us/frame  draw {:>10.3} us/frame",
        collect_elapsed.as_secs_f64() * 1_000_000.0 / frames as f64,
        line_elapsed.as_secs_f64() * 1_000_000.0 / frames as f64,
        rail_elapsed.as_secs_f64() * 1_000_000.0 / frames as f64,
        draw_elapsed.as_secs_f64() * 1_000_000.0 / frames as f64,
    );
    print(
        &format!("disasm render x86_64 {name} frames"),
        elapsed,
        frames,
    );
    Ok(())
}

#[cfg(feature = "disasm-yaxpeax-arm")]
fn aarch64_elf_bytes(code: &[u8]) -> Vec<u8> {
    let text_virtual = 0x401000u64;
    let mut bytes = vec![0_u8; (0x100 + code.len()).max(0x200)];
    bytes[0..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&183u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x100u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    let ph = 64usize;
    bytes[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[ph + 4..ph + 8].copy_from_slice(&0x5u32.to_le_bytes());
    bytes[ph + 8..ph + 16].copy_from_slice(&0x100u64.to_le_bytes());
    bytes[ph + 16..ph + 24].copy_from_slice(&text_virtual.to_le_bytes());
    bytes[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
    bytes[0x100..0x100 + code.len()].copy_from_slice(code);
    bytes
}

#[cfg(feature = "disasm-yaxpeax-arm")]
fn bench_disasm_decode_aarch64_ret_rows() -> BenchResult {
    let dir = tempdir()?;
    let path = dir.path().join("disasm-decode-aarch64-ret.bin");
    let mut code = Vec::with_capacity(64 * 1024 * 4);
    for _ in 0..64 * 1024 {
        code.extend_from_slice(&[0xc0, 0x03, 0x5f, 0xd6]);
    }
    fs::write(&path, aarch64_elf_bytes(&code))?;

    let mut doc = Document::open(&path, &bench_config())?;
    let info = hxedit::executable::detect_executable_info(&mut doc)
        .ok_or_else(|| std::io::Error::other("synthetic ELF was not detected"))?;
    let backend = hxedit::disasm::backend::resolve_backend(&info, None)?;

    let t = Instant::now();
    let rows = hxedit::disasm::decode_region_rows(
        &mut doc,
        &info,
        backend.as_ref(),
        0x100,
        code.len() / 4,
    )?;
    let elapsed = t.elapsed();
    assert_eq!(rows.len(), code.len() / 4);
    assert!(rows.iter().all(|row| row.text == "ret"));
    print("disasm decode aarch64 ret rows", elapsed, rows.len());
    Ok(())
}

#[cfg(not(feature = "disasm-yaxpeax-arm"))]
fn bench_disasm_decode_aarch64_ret_rows() -> BenchResult {
    eprintln!("[bench] disasm decode aarch64 skipped: disasm-yaxpeax-arm feature disabled");
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

fn bench_edit_256mb_mixed_paste_overwrite_64mb() -> BenchResult {
    let (_dir, path) =
        write_sparse_zero_file("edit-256-mixed-paste-overwrite.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.overwrite_run_pattern_overlay(0, EDIT_256_BULK_BYTES as u64, &[0x7f])?;
    assert!(!doc.replacement_range_is_pristine(0, EDIT_256_BULK_BYTES as u64));
    let bytes = vec![1_u8; EDIT_256_BULK_BYTES];

    let t = Instant::now();
    let (written, runs) = doc.overwrite_run_bytes_overlay_changed(0, &bytes)?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_256_BULK_BYTES as u64);
    assert_eq!(runs.len(), 1);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);
    print(
        "edit 256MB mixed paste overwrite 64MB",
        elapsed,
        EDIT_256_BULK_BYTES,
    );
    Ok(())
}

fn bench_edit_256mb_mixed_fill_overlay_64mb() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-mixed-fill.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.overwrite_run_pattern_overlay(0, EDIT_256_BULK_BYTES as u64, &[0x11])?;
    assert!(!doc.replacement_range_is_pristine(0, EDIT_256_BULK_BYTES as u64));

    let t = Instant::now();
    let stats = doc.overwrite_run_pattern_overlay(0, EDIT_256_BULK_BYTES as u64, &[0x22, 0x33])?;
    let elapsed = t.elapsed();
    assert_eq!(stats.visited, EDIT_256_BULK_BYTES as u64);
    assert_eq!(stats.changed, EDIT_256_BULK_BYTES as u64);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);
    print(
        "edit 256MB mixed fill 64MB overlay",
        elapsed,
        EDIT_256_BULK_BYTES,
    );
    Ok(())
}

fn bench_edit_256mb_mixed_xor_overlay_64mb() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("edit-256-mixed-xor.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    doc.overwrite_run_pattern_overlay(0, EDIT_256_BULK_BYTES as u64, &[0x11, 0x22])?;
    assert!(!doc.replacement_range_is_pristine(0, EDIT_256_BULK_BYTES as u64));

    let t = Instant::now();
    let stats = doc.xor_visible_range_mixed_overlay(0, EDIT_256_BULK_BYTES as u64 - 1, 0x5a)?;
    let elapsed = t.elapsed();
    assert_eq!(stats.visited, EDIT_256_BULK_BYTES as u64);
    assert_eq!(stats.changed, EDIT_256_BULK_BYTES as u64);
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);
    print(
        "edit 256MB mixed xor 64MB overlay",
        elapsed,
        EDIT_256_BULK_BYTES,
    );
    Ok(())
}

fn bench_session_256mb_mixed_10k_ops() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("session-256-mixed.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let mut rng = Lcg::new(0x7b8d_6a5c_4e3f_2910);
    let ops = 10_000usize;

    let t = Instant::now();
    for _ in 0..ops {
        let len = doc.len();
        if len == 0 {
            doc.insert_bytes(0, &[rng.next_byte()])?;
            continue;
        }

        match rng.next_usize(8) {
            0 => {
                let offset = rng.next_usize(len as usize) as u64;
                if !matches!(
                    doc.byte_at(offset)?,
                    hxedit::core::document::ByteSlot::Deleted
                ) {
                    doc.replace_display_byte(offset, rng.next_byte())?;
                }
            }
            1 => {
                if doc.len() < (EDIT_256_FILE_SIZE + 64 * 1024) as u64 {
                    let offset = rng.next_usize(len as usize + 1) as u64;
                    let count = 1 + rng.next_usize(4);
                    let bytes = (0..count).map(|_| rng.next_byte()).collect::<Vec<_>>();
                    doc.insert_bytes(offset, &bytes)?;
                }
            }
            2 => {
                let offset = rng.next_usize(len as usize) as u64;
                let _ = doc.delete_byte(offset)?;
            }
            3 => {
                let offset = rng.next_usize(len as usize) as u64;
                let run_len = (1 + rng.next_usize(4)) as u64;
                let _ = doc.delete_range_real(offset, run_len)?;
            }
            4 => {
                let offset = rng.next_usize(len as usize) as u64;
                let run_len = (1 + rng.next_usize(128)) as u64;
                let pattern = [rng.next_byte(), rng.next_byte().wrapping_add(1)];
                doc.overwrite_run_pattern_overlay(offset, run_len, &pattern)?;
            }
            5 => {
                let offset = rng.next_usize(len as usize) as u64;
                let run_len = (1 + rng.next_usize(128)) as u64;
                let end = offset
                    .saturating_add(run_len)
                    .min(doc.len())
                    .saturating_sub(1);
                doc.xor_visible_range_mixed_overlay(offset, end, rng.next_byte() | 1)?;
            }
            6 => {
                let offset = rng.next_usize(len as usize) as u64;
                let applied = (1 + rng.next_usize(64)).min((doc.len() - offset) as usize);
                if !display_range_has_tombstone(&doc, offset, applied as u64) {
                    let bytes = (0..applied).map(|_| rng.next_byte()).collect::<Vec<_>>();
                    let _ = doc.overwrite_run_bytes_overlay_changed(offset, &bytes)?;
                }
            }
            _ => {
                let offset = rng.next_usize(len as usize) as u64;
                let run_len = (1 + rng.next_usize(256)) as u64;
                let end = offset
                    .saturating_add(run_len)
                    .min(doc.len())
                    .saturating_sub(1);
                let _ = doc.logical_byte_count(offset, end)?;
            }
        }
    }
    let elapsed = t.elapsed();

    assert!(!doc.is_empty());
    assert!(doc.visible_len() <= doc.len());
    print("session 256MB mixed 10k core ops", elapsed, ops);
    Ok(())
}

fn bench_undo_redo_64mb_compact_paste() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("undo-redo-64mb-compact.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = vec![1_u8; EDIT_256_BULK_BYTES];

    let t = Instant::now();
    let (written, runs) = doc.overwrite_run_bytes_overlay_changed(0, &bytes)?;
    let apply_elapsed = t.elapsed();
    assert_eq!(written, EDIT_256_BULK_BYTES as u64);
    assert_eq!(runs.len(), 1);
    print(
        "undo/redo 64MB compact paste apply",
        apply_elapsed,
        EDIT_256_BULK_BYTES,
    );

    let t = Instant::now();
    doc.clear_replacements_in_display_range(0, written)?;
    let undo_elapsed = t.elapsed();
    assert_eq!(doc.replacement_dirty_bytes(), 0);
    print(
        "undo/redo 64MB compact paste undo",
        undo_elapsed,
        EDIT_256_BULK_BYTES,
    );

    let t = Instant::now();
    for (offset, bytes) in &runs {
        doc.overwrite_run_bytes_overlay(*offset, Arc::clone(bytes))?;
    }
    let redo_elapsed = t.elapsed();
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);
    print(
        "undo/redo 64MB compact paste redo",
        redo_elapsed,
        EDIT_256_BULK_BYTES,
    );
    Ok(())
}

fn bench_undo_redo_4mb_per_byte_fallback() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("undo-redo-4mb-per-byte.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let bytes = vec![1_u8; EDIT_PER_BYTE_LARGE_BYTES];

    let t = Instant::now();
    let (written, changes) =
        doc.overwrite_run_positional(0, bytes.len() as u64, |idx| bytes[idx as usize])?;
    let apply_elapsed = t.elapsed();
    assert_eq!(written, EDIT_PER_BYTE_LARGE_BYTES as u64);
    assert_eq!(changes.len(), EDIT_PER_BYTE_LARGE_BYTES);
    print(
        "undo/redo 4MB per-byte fallback apply",
        apply_elapsed,
        EDIT_PER_BYTE_LARGE_BYTES,
    );

    let t = Instant::now();
    for (id, before, _) in &changes {
        doc.restore_replacement(*id, *before)?;
    }
    let undo_elapsed = t.elapsed();
    assert_eq!(doc.replacement_dirty_bytes(), 0);
    print(
        "undo/redo 4MB per-byte fallback undo",
        undo_elapsed,
        EDIT_PER_BYTE_LARGE_BYTES,
    );

    let t = Instant::now();
    for (id, _, after) in &changes {
        doc.restore_replacement(*id, *after)?;
    }
    let redo_elapsed = t.elapsed();
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_PER_BYTE_LARGE_BYTES);
    print(
        "undo/redo 4MB per-byte fallback redo",
        redo_elapsed,
        EDIT_PER_BYTE_LARGE_BYTES,
    );
    Ok(())
}

fn bench_dirty_islands_paste_64mb() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("dirty-islands-paste.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let islands = 4096usize;
    for i in 0..islands {
        let offset = ((i * 16_381) % EDIT_256_BULK_BYTES) as u64;
        doc.replace_display_byte(offset, (i as u8).wrapping_add(1))?;
    }
    assert!(!doc.replacement_range_is_pristine(0, EDIT_256_BULK_BYTES as u64));

    let bytes = vec![0x5a; EDIT_256_BULK_BYTES];
    let t = Instant::now();
    let (written, runs) = doc.overwrite_run_bytes_overlay_changed(0, &bytes)?;
    let elapsed = t.elapsed();
    assert_eq!(written, EDIT_256_BULK_BYTES as u64);
    assert!(!runs.is_empty());
    assert_eq!(doc.replacement_dirty_bytes(), EDIT_256_BULK_BYTES);
    print(
        "dirty islands paste overwrite 64MB",
        elapsed,
        EDIT_256_BULK_BYTES,
    );
    Ok(())
}

fn bench_dirty_islands_xor_64mb() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("dirty-islands-xor.bin", EDIT_256_FILE_SIZE)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let islands = 4096usize;
    for i in 0..islands {
        let offset = (i * (EDIT_256_BULK_BYTES / islands)) as u64;
        if i % 4 == 0 {
            let _ = doc.delete_byte(offset)?;
        } else {
            doc.replace_display_byte(offset, 0x11)?;
        }
    }
    assert!(display_range_has_tombstone(
        &doc,
        0,
        EDIT_256_BULK_BYTES as u64
    ));

    let t = Instant::now();
    let stats = doc.xor_visible_range_mixed_overlay(0, EDIT_256_BULK_BYTES as u64 - 1, 0x5a)?;
    let elapsed = t.elapsed();
    assert_eq!(
        stats.visited,
        EDIT_256_BULK_BYTES as u64 - (islands / 4) as u64
    );
    assert_eq!(stats.changed, stats.visited);
    print("dirty islands xor 64MB", elapsed, EDIT_256_BULK_BYTES);
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

fn bench_open_1gib_sparse() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("open-1gib-sparse.bin", LARGE_FILE_SIZE_1GIB)?;
    let config = bench_config();

    let t = Instant::now();
    let doc = Document::open(&path, &config)?;
    let elapsed = t.elapsed();
    assert_eq!(doc.len(), LARGE_FILE_SIZE_1GIB as u64);
    print("open 1GiB sparse file", elapsed, 1);
    Ok(())
}

fn bench_open_1gib_then_first_view() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("open-1gib-first-view.bin", LARGE_FILE_SIZE_1GIB)?;
    let config = bench_config();

    let t = Instant::now();
    let mut doc = Document::open(&path, &config)?;
    let rows = 64usize;
    for row in 0..rows {
        let bytes = doc.row_bytes((row * 16) as u64, 16)?;
        assert_eq!(bytes.len(), 16);
    }
    let elapsed = t.elapsed();
    print("open 1GiB sparse file + first view", elapsed, rows);
    Ok(())
}

fn bench_viewport_1gib_random_10k_rows() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("viewport-1gib-random.bin", LARGE_FILE_SIZE_1GIB)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let mut rng = Lcg::new(0x9f3a_76bc_5812_de40);
    let rows = 10_000usize;
    let max_row = (doc.len() / 16) as usize;

    let t = Instant::now();
    for _ in 0..rows {
        let row = rng.next_usize(max_row);
        let bytes = doc.row_bytes((row * 16) as u64, 16)?;
        assert_eq!(bytes.len(), 16);
    }
    let elapsed = t.elapsed();
    print("viewport 1GiB random 10k rows", elapsed, rows);
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

fn bench_export_stream_256mb_patterned() -> BenchResult {
    let (_dir, path) = write_patterned_file("export-256-patterned.bin", PATTERNED_FILE_SIZE_256MB)?;
    let mut doc = Document::open(&path, &bench_config())?;
    let out_dir = tempdir()?;
    let out_path = out_dir.path().join("export-256-out.bin");

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
    assert_eq!(written, PATTERNED_FILE_SIZE_256MB as u64);
    print("export stream 256MB patterned file", elapsed, 1);
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
    // forcing a full scan. Use a sparse fixture so peak RSS measures the search
    // path instead of a benchmark-side 256 MiB initialization buffer.
    let size: usize = 256 * 1024 * 1024;
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let (_dir, path, offset) =
        write_sparse_file_with_tail_needle("search-256-clean.bin", size, &needle)?;

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
    let size: usize = 256 * 1024 * 1024;
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let (_dir, path, offset) =
        write_sparse_file_with_tail_needle("search-256-dirty.bin", size, &needle)?;

    let mut doc = Document::open(&path, &bench_config())?;
    doc.delete_byte(5)?; // tombstone near the start -> dirty path

    let t = Instant::now();
    let found = doc.search_forward(0, &needle)?;
    let elapsed = t.elapsed();
    assert_eq!(found, Some(offset as u64)); // tombstones keep their display slot
    print("search 256MB dirty(1 tombstone) forward", elapsed, 1);
    Ok(())
}

fn bench_search_256mb_clean_sparse_fixture() -> BenchResult {
    let size: usize = 256 * 1024 * 1024;
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let (_dir, path, offset) =
        write_sparse_file_with_tail_needle("search-256-clean-sparse.bin", size, &needle)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let found = doc.search_forward(0, &needle)?;
    let elapsed = t.elapsed();
    assert_eq!(found, Some(offset as u64));
    print("search 256MB clean sparse fixture", elapsed, 1);
    Ok(())
}

fn bench_search_256mb_dirty_many_islands() -> BenchResult {
    let size: usize = 256 * 1024 * 1024;
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let (_dir, path, offset) =
        write_sparse_file_with_tail_needle("search-256-dirty-islands.bin", size, &needle)?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..4096usize {
        let dirty_offset = (i * 16_384) as u64;
        if i % 8 == 0 {
            let _ = doc.delete_byte(dirty_offset)?;
        } else {
            doc.replace_display_byte(dirty_offset, 0x5a)?;
        }
    }

    let t = Instant::now();
    let found = doc.search_forward(0, &needle)?;
    let elapsed = t.elapsed();
    assert_eq!(found, Some(offset as u64));
    print("search 256MB dirty many islands", elapsed, 1);
    Ok(())
}

fn bench_search_1gib_clean_memmem() -> BenchResult {
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let (_dir, path, offset) =
        write_sparse_file_with_tail_needle("search-1gib-clean.bin", LARGE_FILE_SIZE_1GIB, &needle)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let found = doc.search_forward(0, &needle)?;
    let elapsed = t.elapsed();
    assert_eq!(found, Some(offset as u64));
    print("search 1GiB clean forward (memmem)", elapsed, 1);
    Ok(())
}

fn bench_search_1gib_dirty_many_islands() -> BenchResult {
    let needle = [0xde, 0xad, 0xbe, 0xef];
    let (_dir, path, offset) = write_sparse_file_with_tail_needle(
        "search-1gib-dirty-islands.bin",
        LARGE_FILE_SIZE_1GIB,
        &needle,
    )?;
    let mut doc = Document::open(&path, &bench_config())?;
    for i in 0..16_384usize {
        let dirty_offset = (i * 16_384) as u64;
        if i % 8 == 0 {
            let _ = doc.delete_byte(dirty_offset)?;
        } else {
            doc.replace_display_byte(dirty_offset, 0x5a)?;
        }
    }

    let t = Instant::now();
    let found = doc.search_forward(0, &needle)?;
    let elapsed = t.elapsed();
    assert_eq!(found, Some(offset as u64));
    print("search 1GiB dirty many islands", elapsed, 1);
    Ok(())
}

fn bench_hash_crc32_1gib_clean() -> BenchResult {
    let (_dir, path) = write_sparse_zero_file("hash-crc32-1gib.bin", LARGE_FILE_SIZE_1GIB)?;
    let mut doc = Document::open(&path, &bench_config())?;

    let t = Instant::now();
    let (bytes_hashed, hash_bytes) =
        doc.hash_logical_bytes(0, doc.len() - 1, make_hasher(HashAlgorithm::Crc32))?;
    let elapsed = t.elapsed();
    assert_eq!(bytes_hashed, LARGE_FILE_SIZE_1GIB as u64);
    assert_eq!(hash_bytes.len(), 4);
    print("hash 1GiB crc32 clean sparse file", elapsed, 1);
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

fn bench_diff_next_tail_mismatch_1gib_stepper() -> BenchResult {
    let size = LARGE_FILE_SIZE_1GIB;
    let (_dir, current, other) = write_sparse_diff_pair("diff-next-1gib-stepper", size)?;
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
    print("diff next tail mismatch 1GiB stepper", elapsed, steps);
    Ok(())
}

fn bench_diff_next_tail_mismatch_1gib_dirty_stepper() -> BenchResult {
    let size = LARGE_FILE_SIZE_1GIB;
    let (_dir, current, other) = write_sparse_diff_pair("diff-next-1gib-dirty-stepper", size)?;
    let config = bench_config();
    let mut document = Document::open(&current, &config)?;
    let mut other_file = fs::OpenOptions::new().write(true).open(&other)?;

    for i in 0..16_384usize {
        let dirty_offset = (i * 16_384) as u64;
        document.replace_display_byte(dirty_offset, 0x5a)?;
        other_file.seek(SeekFrom::Start(dirty_offset))?;
        other_file.write_all(&[0x5a])?;
    }
    other_file.flush()?;
    assert!(document.has_replacements());
    assert!(!document.has_tombstones());

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

    assert_eq!(found, Some(size as u64 - 1));
    print("diff next tail mismatch 1GiB dirty stepper", elapsed, steps);
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
    let t = Instant::now();
    let success = match bench() {
        Ok(()) => true,
        Err(err) => {
            eprintln!("[bench] {label} failed: {err}");
            false
        }
    };
    eprintln!(
        "[bench] {label:<48} process-total {:>12.6} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
    print_peak_rss(label);
    success
}

fn run_isolated(label: &str) -> Option<BenchMeasurement> {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            eprintln!("[bench] {label} failed: unable to resolve current executable: {err}");
            return None;
        }
    };

    let output = match Command::new(exe)
        .env(BENCH_CHILD_ENV, label)
        .env("HXEDIT_RUN_BENCH", "1")
        .output()
    {
        Err(err) => {
            eprintln!("[bench] {label} failed: unable to spawn isolated child: {err}");
            return None;
        }
        Ok(output) => output,
    };

    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    print!("{}", String::from_utf8_lossy(&output.stdout));

    if !output.status.success() {
        return None;
    }

    parse_child_measurement(&output.stderr)
}

fn parse_child_measurement(stderr: &[u8]) -> Option<BenchMeasurement> {
    let stderr = String::from_utf8_lossy(stderr);
    let mut elapsed_ms = None;
    let mut peak_rss = None;

    for line in stderr.lines() {
        if let Some((_, rest)) = line.split_once(" total ") {
            if let Some((value, _)) = rest.trim_start().split_once(" ms") {
                elapsed_ms = value.trim().parse::<f64>().ok();
            }
        }
        if let Some((_, value)) = line.split_once(" peak-rss ") {
            peak_rss = parse_bytes(value.trim());
        }
    }

    elapsed_ms.map(|elapsed_ms| BenchMeasurement {
        elapsed_ms,
        peak_rss,
    })
}

fn parse_bytes(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number = parts.next()?.parse::<f64>().ok()?;
    let unit = parts.next()?;
    let multiplier = match unit {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * multiplier).round() as u64)
}

fn default_benches() -> &'static [BenchEntry] {
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
        (
            "disasm_decode_x86_64_nop_rows",
            bench_disasm_decode_x86_64_nop_rows,
        ),
        (
            "disasm_decode_aarch64_ret_rows",
            bench_disasm_decode_aarch64_ret_rows,
        ),
        (
            "disasm_render_x86_64_nop_frames",
            bench_disasm_render_x86_64_nop_frames,
        ),
        (
            "disasm_render_x86_64_nop_rail_frames",
            bench_disasm_render_x86_64_nop_rail_frames,
        ),
        (
            "disasm_render_x86_64_jmp_frames",
            bench_disasm_render_x86_64_jmp_frames,
        ),
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
            "edit_mode_paste_overwrite_16mb",
            bench_edit_mode_paste_overwrite_16mb,
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
            "edit_256mb_paste_insert_16mb",
            bench_edit_256mb_paste_insert_16mb,
        ),
        (
            "edit_256mb_fill_overlay_64mb",
            bench_edit_256mb_fill_overlay_64mb,
        ),
        (
            "edit_256mb_mixed_paste_overwrite_64mb",
            bench_edit_256mb_mixed_paste_overwrite_64mb,
        ),
        (
            "edit_256mb_mixed_fill_overlay_64mb",
            bench_edit_256mb_mixed_fill_overlay_64mb,
        ),
        (
            "edit_256mb_mixed_xor_overlay_64mb",
            bench_edit_256mb_mixed_xor_overlay_64mb,
        ),
        (
            "session_256mb_mixed_10k_ops",
            bench_session_256mb_mixed_10k_ops,
        ),
        (
            "undo_redo_64mb_compact_paste",
            bench_undo_redo_64mb_compact_paste,
        ),
        ("dirty_islands_paste_64mb", bench_dirty_islands_paste_64mb),
        ("dirty_islands_xor_64mb", bench_dirty_islands_xor_64mb),
        ("logical_bytes_large_copy", bench_logical_bytes_large_copy),
        ("export_stream_64mb", bench_export_stream_64mb),
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
            "search_256mb_dirty_many_islands",
            bench_search_256mb_dirty_many_islands,
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

fn legacy_benches() -> &'static [BenchEntry] {
    &[
        ("paste_overwrite_large", bench_paste_overwrite_large),
        ("paste_overwrite_bulk_path", bench_paste_overwrite_bulk_path),
        (
            "edit_mode_paste_overwrite_per_byte_1mb",
            bench_edit_mode_paste_overwrite_per_byte_1mb,
        ),
        (
            "edit_mode_paste_overwrite_per_byte_4mb",
            bench_edit_mode_paste_overwrite_per_byte_4mb,
        ),
        (
            "edit_256mb_paste_overwrite_per_byte_8mb",
            bench_edit_256mb_paste_overwrite_per_byte_8mb,
        ),
        (
            "undo_redo_4mb_per_byte_fallback",
            bench_undo_redo_4mb_per_byte_fallback,
        ),
        (
            "search_256mb_clean_sparse_fixture",
            bench_search_256mb_clean_sparse_fixture,
        ),
        ("fill_stream_4mb", bench_fill_stream_4mb),
        ("xor_stream_4mb", bench_xor_stream_4mb),
    ]
}

fn large_benches() -> &'static [BenchEntry] {
    &[
        ("open_1gib_sparse", bench_open_1gib_sparse),
        ("open_1gib_then_first_view", bench_open_1gib_then_first_view),
        (
            "viewport_1gib_random_10k_rows",
            bench_viewport_1gib_random_10k_rows,
        ),
        (
            "save_256mb_patterned_clean_rewrite",
            bench_save_256mb_patterned_clean_rewrite,
        ),
        (
            "export_stream_256mb_patterned",
            bench_export_stream_256mb_patterned,
        ),
        (
            "save_1gib_with_middle_insert",
            bench_save_1gib_with_middle_insert,
        ),
        (
            "save_1gib_with_tombstone_and_insert",
            bench_save_1gib_with_tombstone_and_insert,
        ),
        (
            "save_1gib_with_64mb_range_overlay",
            bench_save_1gib_with_64mb_range_overlay,
        ),
        (
            "save_1gib_with_sparse_replacement_islands",
            bench_save_1gib_with_sparse_replacement_islands,
        ),
        ("search_1gib_clean_memmem", bench_search_1gib_clean_memmem),
        (
            "search_1gib_dirty_many_islands",
            bench_search_1gib_dirty_many_islands,
        ),
        ("hash_crc32_1gib_clean", bench_hash_crc32_1gib_clean),
        (
            "diff_next_tail_mismatch_1gib_stepper",
            bench_diff_next_tail_mismatch_1gib_stepper,
        ),
        (
            "diff_next_tail_mismatch_1gib_dirty_stepper",
            bench_diff_next_tail_mismatch_1gib_dirty_stepper,
        ),
    ]
}

fn public_benches() -> &'static [BenchEntry] {
    &[
        ("open_1gib_sparse", bench_open_1gib_sparse),
        ("open_1gib_then_first_view", bench_open_1gib_then_first_view),
        (
            "viewport_1gib_random_10k_rows",
            bench_viewport_1gib_random_10k_rows,
        ),
        (
            "save_256mb_patterned_clean_rewrite",
            bench_save_256mb_patterned_clean_rewrite,
        ),
        (
            "export_stream_256mb_patterned",
            bench_export_stream_256mb_patterned,
        ),
        (
            "save_1gib_with_middle_insert",
            bench_save_1gib_with_middle_insert,
        ),
        (
            "save_1gib_with_tombstone_and_insert",
            bench_save_1gib_with_tombstone_and_insert,
        ),
        (
            "save_1gib_with_sparse_replacement_islands",
            bench_save_1gib_with_sparse_replacement_islands,
        ),
        ("search_1gib_clean_memmem", bench_search_1gib_clean_memmem),
        (
            "search_1gib_dirty_many_islands",
            bench_search_1gib_dirty_many_islands,
        ),
        (
            "diff_next_tail_mismatch_1gib_stepper",
            bench_diff_next_tail_mismatch_1gib_stepper,
        ),
        (
            "diff_next_tail_mismatch_1gib_dirty_stepper",
            bench_diff_next_tail_mismatch_1gib_dirty_stepper,
        ),
        (
            "session_256mb_mixed_10k_ops",
            bench_session_256mb_mixed_10k_ops,
        ),
    ]
}

fn bench_by_label(label: &str) -> Option<BenchEntry> {
    default_benches()
        .iter()
        .chain(legacy_benches().iter())
        .chain(large_benches().iter())
        .chain(public_benches().iter())
        .copied()
        .find(|(entry_label, _)| *entry_label == label)
}

fn repeat_count() -> usize {
    std::env::var(BENCH_REPEAT_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn print_repeat_summary(label: &str, measurements: &[BenchMeasurement]) {
    if measurements.len() <= 1 {
        return;
    }
    let mut elapsed = measurements
        .iter()
        .map(|measurement| measurement.elapsed_ms)
        .collect::<Vec<_>>();
    elapsed.sort_by(f64::total_cmp);
    let min = elapsed[0];
    let median = elapsed[elapsed.len() / 2];
    let max = elapsed[elapsed.len() - 1];
    let peak_rss = measurements.iter().filter_map(|m| m.peak_rss).max();
    let peak = peak_rss
        .map(format_bytes)
        .unwrap_or_else(|| "child-reported".to_owned());
    eprintln!(
        "[bench] {label:<48} repeat-summary min {min:>12.6} ms  median {median:>12.6} ms  max {max:>12.6} ms  peak-rss {peak}"
    );
}

fn run_repeated(label: &str, repeat: usize) -> bool {
    let mut measurements = Vec::with_capacity(repeat);
    for run in 0..repeat {
        if repeat > 1 {
            eprintln!("[bench] repeat {}/{} {label}", run + 1, repeat);
        }
        let Some(measurement) = run_isolated(label) else {
            return false;
        };
        measurements.push(measurement);
    }
    print_repeat_summary(label, &measurements);
    true
}

fn active_benches() -> Vec<BenchEntry> {
    match std::env::var(BENCH_SUITE_ENV).as_deref() {
        Ok("public") => public_benches().to_vec(),
        _ => {
            let include_legacy = std::env::var_os("HXEDIT_BENCH_LEGACY").is_some();
            let include_large = std::env::var_os("HXEDIT_BENCH_LARGE").is_some();
            let mut active = default_benches().to_vec();
            if include_legacy {
                active.extend_from_slice(legacy_benches());
            }
            if include_large {
                active.extend_from_slice(large_benches());
            }
            active
        }
    }
}

fn main() {
    if let Some(child_label) = std::env::var_os(BENCH_CHILD_ENV) {
        let child_label = child_label.to_string_lossy();
        let Some((label, bench)) = bench_by_label(child_label.as_ref()) else {
            eprintln!("[bench] unknown child bench {child_label}");
            std::process::exit(1);
        };
        if !run_in_process(label, bench) {
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
    let repeat = repeat_count();
    let active_benches = active_benches();
    let failed = active_benches
        .iter()
        .filter(|(label, _)| {
            filter
                .as_deref()
                .is_none_or(|needle| label.contains(needle))
        })
        .filter(|(label, _)| !run_repeated(label, repeat))
        .count();
    if failed > 0 {
        std::process::exit(1);
    }
}
