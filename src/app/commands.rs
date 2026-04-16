use crate::app::{App, SearchDirection, SearchKind, SearchState};
use crate::commands::parser::parse_command;
use crate::commands::types::{Command, GotoTarget, HashAlgorithm};
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

impl App {
    pub(crate) fn submit_command(&mut self) -> HxResult<()> {
        let return_mode = self.command_return_mode.unwrap_or(Mode::Normal);
        let command = parse_command(&self.command_buffer)?;
        self.execute_command(command)?;
        self.remember_command_submission();
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        if matches!(self.mode, Mode::Command) {
            self.mode = self.normalize_mode(return_mode);
        }
        self.command_return_mode = None;
        self.reset_command_history_navigation();
        Ok(())
    }

    pub(crate) fn execute_command(&mut self, command: Command) -> HxResult<()> {
        match command {
            Command::Quit { force } => self.execute_quit_command(force),
            Command::Write { path } => self.execute_write_command(path, false),
            Command::WriteQuit { path } => self.execute_write_command(path, true),
            Command::Goto { target } => self.execute_goto_command(target),
            Command::Undo { steps } => self.undo(steps, false),
            Command::Redo { steps } => self.redo(steps, false),
            Command::Paste {
                raw,
                preview,
                limit,
            } => self.execute_paste_command(raw, preview, limit, false),
            Command::PasteInsert {
                raw,
                preview,
                limit,
            } => self.execute_paste_command(raw, preview, limit, true),
            Command::Copy { format, display } => self.copy_selection(format, display),
            Command::Inspector => {
                self.execute_inspector_command();
                Ok(())
            }
            Command::Format { name } => {
                self.execute_format_command(name);
                Ok(())
            }
            Command::SearchAscii { pattern, backward } => {
                self.execute_search_command(SearchKind::Ascii, pattern, backward)
            }
            Command::SearchHex { pattern, backward } => {
                self.execute_search_command(SearchKind::Hex, pattern, backward)
            }
            Command::Hash { algorithm } => self.execute_hash_command(algorithm),
        }
    }

    fn execute_quit_command(&mut self, force: bool) -> HxResult<()> {
        if self.document.is_dirty() && !force {
            return Err(HxError::DirtyQuit);
        }
        self.should_quit = true;
        Ok(())
    }

    fn execute_write_command(
        &mut self,
        path: Option<std::path::PathBuf>,
        should_quit: bool,
    ) -> HxResult<()> {
        let (saved, profile) = self.document.save(path)?;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.cursor = self.clamp_offset(self.cursor);
        self.refresh_inspector();
        self.set_info_status(format!("wrote {} [{}]", saved.display(), profile));
        self.should_quit = should_quit;
        Ok(())
    }

    fn execute_goto_command(&mut self, target: GotoTarget) -> HxResult<()> {
        let offset = self.resolve_goto_target(target)?;
        self.cursor = self.document.goto(offset)?;
        self.set_info_status(format!("goto 0x{:x}", self.cursor));
        Ok(())
    }

    fn resolve_goto_target(&self, target: GotoTarget) -> HxResult<u64> {
        match target {
            GotoTarget::Absolute(offset) => Ok(offset),
            GotoTarget::End => {
                if self.document.is_empty() {
                    Ok(0)
                } else {
                    Ok(self.document.len() - 1)
                }
            }
            GotoTarget::Relative(delta) => {
                let current = i64::try_from(self.cursor)
                    .map_err(|_| HxError::InvalidOffset(delta.to_string()))?;
                let target = current.saturating_add(delta);
                u64::try_from(target).map_err(|_| HxError::OffsetOutOfRange)
            }
        }
    }

    fn execute_paste_command(
        &mut self,
        raw: bool,
        preview: bool,
        limit: Option<usize>,
        insert: bool,
    ) -> HxResult<()> {
        self.paste_from_clipboard(raw, preview, limit, insert)
    }

    fn execute_inspector_command(&mut self) {
        let from_inspector = self
            .command_return_mode
            .is_some_and(|mode| mode.is_inspector());
        if !self.show_inspector {
            self.show_inspector = true;
            self.refresh_inspector();
            self.focus_inspector_or_warn();
        } else if !from_inspector {
            self.focus_inspector_or_warn();
        } else {
            self.mode = Mode::Normal;
            self.show_inspector = false;
            self.inspector = None;
            self.inspector_error = None;
        }
    }

    fn execute_format_command(&mut self, name: Option<String>) {
        self.show_inspector = true;
        match name {
            Some(name) => self.execute_named_format_command(name),
            None => {
                self.inspector_format_override = None;
                self.refresh_inspector();
                if self.focus_inspector_or_warn() {
                    self.set_info_status("format: auto");
                }
            }
        }
    }

    fn execute_named_format_command(&mut self, name: String) {
        if crate::format::detect::detect_by_name(&name, &mut self.document).is_some() {
            self.inspector_format_override = Some(name.to_lowercase());
            self.refresh_inspector();
            if self.focus_inspector_or_warn() {
                self.set_info_status(format!("format: {}", name));
            }
        } else {
            self.set_error_status(format!("unknown or mismatched format: {}", name));
        }
    }

    fn execute_search_command(
        &mut self,
        kind: SearchKind,
        pattern: Vec<u8>,
        backward: bool,
    ) -> HxResult<()> {
        let search = SearchState { kind, pattern };
        self.last_search = Some(search.clone());
        self.run_search(&search, search_direction(backward))
    }

    fn execute_hash_command(&mut self, algorithm: HashAlgorithm) -> HxResult<()> {
        let (start, end) = if let Some((start, end)) = self.selection_range() {
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
        let scope = if self.selection_range().is_some() {
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
}

fn search_direction(backward: bool) -> SearchDirection {
    if backward {
        SearchDirection::Backward
    } else {
        SearchDirection::Forward
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
