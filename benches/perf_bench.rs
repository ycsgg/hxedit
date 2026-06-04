//! Performance benchmark harness.
//!
//! Run with: `cargo bench --bench perf_bench`

use std::fs;
use std::io::Write;
use std::time::{Duration, Instant};

use digest::Digest;
use hxedit::commands::types::HashAlgorithm;
use hxedit::config::Config;
use hxedit::core::document::Document;
use hxedit::format;
use tempfile::tempdir;

type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type BenchFn = fn() -> BenchResult;
type BenchEntry = (&'static str, BenchFn);

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

fn write_patterned_file(
    name: &str,
    size: usize,
) -> BenchResult<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempdir()?;
    let path = dir.path().join(name);
    fs::write(&path, patterned_data(size))?;
    Ok((dir, path))
}

fn print(label: &str, elapsed: Duration, unit_count: usize) {
    let ns = elapsed.as_nanos();
    let per = ns as f64 / unit_count.max(1) as f64;
    eprintln!("[bench] {label:<48} total {ns:>12} ns  per-op {per:>10.1} ns  (N={unit_count})");
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

fn run(label: &str, bench: fn() -> BenchResult) -> bool {
    eprintln!("[bench] running {label}");
    match bench() {
        Ok(()) => true,
        Err(err) => {
            eprintln!("[bench] {label} failed: {err}");
            false
        }
    }
}

fn main() {
    if cfg!(debug_assertions) && std::env::var_os("HXEDIT_RUN_BENCH").is_none() {
        eprintln!(
            "[bench] skipped in debug/test profile; run `cargo bench --bench perf_bench` for timings"
        );
        return;
    }

    let benches: &[BenchEntry] = &[
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
        ("logical_bytes_large_copy", bench_logical_bytes_large_copy),
        ("export_stream_64mb", bench_export_stream_64mb),
        ("fill_stream_4mb", bench_fill_stream_4mb),
        ("xor_stream_4mb", bench_xor_stream_4mb),
        ("hash_sha256_16mb", bench_hash_sha256_16mb),
        ("hash_crc32_16mb", bench_hash_crc32_16mb),
        ("hash_16mb_with_tombstones", bench_hash_16mb_with_tombstones),
        ("hash_16mb_with_insert", bench_hash_16mb_with_insert),
        ("search_16mb_file", bench_search_16mb_file),
    ];

    let failed = benches
        .iter()
        .filter(|(label, bench)| !run(label, *bench))
        .count();
    if failed > 0 {
        std::process::exit(1);
    }
}
