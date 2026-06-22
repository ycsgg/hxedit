use crate::commands::types::HashAlgorithm;
use crate::core::document::Document;
use crate::error::HxResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecHash {
    pub algorithm: HashAlgorithm,
    pub bytes_hashed: u64,
    pub hex: String,
}

pub fn hash_display_range(
    document: &mut Document,
    algorithm: HashAlgorithm,
    start: u64,
    end_inclusive: u64,
) -> HxResult<ExecHash> {
    let hasher = make_hasher(algorithm);
    let (bytes_hashed, hash_bytes) = document.hash_logical_bytes(start, end_inclusive, hasher)?;
    let hex = hash_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(ExecHash {
        algorithm,
        bytes_hashed,
        hex,
    })
}

pub(crate) fn make_hasher(algorithm: HashAlgorithm) -> Box<dyn digest::DynDigest> {
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
