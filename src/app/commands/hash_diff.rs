use super::*;

impl App {
    pub(super) fn execute_hash_command(&mut self, algorithm: HashAlgorithm) -> HxResult<()> {
        let selection = self.active_selection_range();
        let (start, end) = if let Some((start, end)) = selection {
            (start, end)
        } else if self.document.is_empty() {
            self.set_info_status(format!("{}: no data to hash", algorithm.label()));
            return Ok(());
        } else {
            (0, self.document.len() - 1)
        };

        let hasher = make_hasher(algorithm);
        let (bytes_hashed, hash_bytes) = self.document.hash_logical_bytes(start, end, hasher)?;

        if bytes_hashed == 0 {
            self.set_info_status(format!("{}: no data to hash", algorithm.label()));
            return Ok(());
        }

        let hash_hex = hash_bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let scope = if selection.is_some() {
            format!("sel 0x{:x}-0x{:x}", start, end)
        } else {
            "entire file".to_owned()
        };

        if crate::clipboard::copy_text(&hash_hex).is_ok() {
            self.set_info_status(format!(
                "{} [{}]: {} ({} bytes) [copied]",
                algorithm.label(),
                scope,
                hash_hex,
                bytes_hashed
            ));
        } else {
            self.set_info_status(format!(
                "{} [{}]: {} ({} bytes)",
                algorithm.label(),
                scope,
                hash_hex,
                bytes_hashed
            ));
        }
        Ok(())
    }

    pub(super) fn execute_diff_command(&mut self, command: DiffCommand) -> HxResult<()> {
        match command {
            DiffCommand::Open { path, max_shift } => self.open_diff_panel(path, max_shift),
            DiffCommand::Refresh => self.refresh_diff_panel(),
            DiffCommand::Next => self.jump_to_next_diff_mismatch(),
            DiffCommand::Prev => self.jump_to_prev_diff_mismatch(),
            DiffCommand::Off => {
                self.close_diff_panel();
                Ok(())
            }
        }
    }

    pub(super) fn close_diff_projection_for_side_panel_switch(&mut self) {
        if self.diff_state().is_some() {
            self.diff_state = None;
            self.clear_diff_cell_selection();
        }
    }
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
