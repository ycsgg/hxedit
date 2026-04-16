use std::fs;
use std::time::Instant;

use hxedit::commands::types::HashAlgorithm;
use hxedit::config::Config;
use hxedit::core::document::Document;
use tempfile::tempdir;

fn make_16mb_doc() -> (tempfile::TempDir, Document) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("hash-perf.bin");
    let size: usize = 16 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let config = Config {
        page_size: 16384,
        cache_pages: 128,
        ..Config::default()
    };
    let doc = Document::open(&path, &config).unwrap();
    (dir, doc)
}

fn make_hasher(algorithm: HashAlgorithm) -> Box<dyn digest::DynDigest> {
    use digest::Digest;
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
        Box::new(Crc32Hasher {
            hasher: self.hasher.clone(),
        })
    }
}

#[test]
fn hash_16mb_sha256_is_fast() {
    let (_dir, mut doc) = make_16mb_doc();

    let hasher = make_hasher(HashAlgorithm::Sha256);
    let start = Instant::now();
    let (bytes_hashed, hash_bytes) = doc.hash_logical_bytes(0, doc.len() - 1, hasher).unwrap();
    let elapsed = start.elapsed();

    eprintln!("16MB sha256 hash took: {elapsed:?}");
    assert!(
        elapsed.as_secs() < 3,
        "sha256 hash took too long: {elapsed:?}"
    );
    assert_eq!(bytes_hashed, 16 * 1024 * 1024);
    assert_eq!(hash_bytes.len(), 32);
}

#[test]
fn hash_16mb_crc32_is_fast() {
    let (_dir, mut doc) = make_16mb_doc();

    let hasher = make_hasher(HashAlgorithm::Crc32);
    let start = Instant::now();
    let (bytes_hashed, hash_bytes) = doc.hash_logical_bytes(0, doc.len() - 1, hasher).unwrap();
    let elapsed = start.elapsed();

    eprintln!("16MB crc32 hash took: {elapsed:?}");
    assert!(
        elapsed.as_secs() < 3,
        "crc32 hash took too long: {elapsed:?}"
    );
    assert_eq!(bytes_hashed, 16 * 1024 * 1024);
    assert_eq!(hash_bytes.len(), 4);
}

#[test]
fn hash_16mb_with_tombstone_is_fast() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("hash-tombstone.bin");
    let size: usize = 16 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let config = Config {
        page_size: 16384,
        cache_pages: 128,
        ..Config::default()
    };
    let mut doc = Document::open(&path, &config).unwrap();

    doc.delete_byte(100).unwrap();
    doc.delete_byte(1_000_000).unwrap();

    let hasher = make_hasher(HashAlgorithm::Sha256);
    let start = Instant::now();
    let (bytes_hashed, _hash_bytes) = doc.hash_logical_bytes(0, doc.len() - 1, hasher).unwrap();
    let elapsed = start.elapsed();

    eprintln!("16MB sha256 (2 tombstones) took: {elapsed:?}");
    assert!(
        elapsed.as_secs() < 3,
        "sha256 with tombstones took too long: {elapsed:?}"
    );
    assert_eq!(bytes_hashed, 16 * 1024 * 1024 - 2);
}

#[test]
fn hash_16mb_with_insert_is_fast() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("hash-insert.bin");
    let size: usize = 16 * 1024 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    fs::write(&path, &data).unwrap();

    let config = Config {
        page_size: 16384,
        cache_pages: 128,
        ..Config::default()
    };
    let mut doc = Document::open(&path, &config).unwrap();

    let mid = size as u64 / 2;
    doc.insert_bytes(mid, &[0xAA, 0xBB]).unwrap();

    let hasher = make_hasher(HashAlgorithm::Md5);
    let start = Instant::now();
    let (bytes_hashed, _hash_bytes) = doc.hash_logical_bytes(0, doc.len() - 1, hasher).unwrap();
    let elapsed = start.elapsed();

    eprintln!("16MB md5 (with insert) took: {elapsed:?}");
    assert!(
        elapsed.as_secs() < 3,
        "md5 with insert took too long: {elapsed:?}"
    );
    assert_eq!(bytes_hashed, 16 * 1024 * 1024 + 2);
}

#[test]
fn logical_bytes_16mb_is_fast() {
    let (_dir, mut doc) = make_16mb_doc();

    let start = Instant::now();
    let bytes = doc.logical_bytes(0, doc.len() - 1).unwrap();
    let elapsed = start.elapsed();

    eprintln!("16MB logical_bytes took: {elapsed:?}");
    assert!(
        elapsed.as_secs() < 3,
        "logical_bytes took too long: {elapsed:?}"
    );
    assert_eq!(bytes.len(), 16 * 1024 * 1024);
}

#[test]
fn hash_selection_range_is_correct() {
    let (_dir, mut doc) = make_16mb_doc();

    let hasher = make_hasher(HashAlgorithm::Sha256);
    let (bytes_hashed, _) = doc.hash_logical_bytes(100, 199, hasher).unwrap();
    assert_eq!(bytes_hashed, 100);
}
