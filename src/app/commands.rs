use crate::app::{App, EditOp, ReplacementChange, SearchDirection, SearchKind, SearchState};
use crate::commands::parser::parse_command;
use crate::commands::types::{Command, ExportFormat, GotoTarget, HashAlgorithm};
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

#[derive(Debug, Clone, Copy)]
struct ReplaceStats {
    match_count: usize,
    before_bytes: usize,
    after_bytes: usize,
    changed_bytes: usize,
}

#[derive(Debug)]
struct ReplaceOutcome {
    ops: Vec<EditOp>,
    stats: ReplaceStats,
}

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
            Command::Fill { pattern, len } => self.execute_fill_command(&pattern, len),
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
            Command::Export { format } => self.execute_export_command(format),
            Command::Replace {
                needle,
                replacement,
                allow_resize,
            } => self.execute_replace_command(&needle, &replacement, allow_resize),
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

    fn execute_fill_command(&mut self, pattern: &[u8], len: usize) -> HxResult<()> {
        if pattern.is_empty() || len == 0 {
            self.set_info_status("fill produced no bytes");
            return Ok(());
        }

        let bytes = repeated_pattern(pattern, len);
        let applied = self.apply_paste_overwrite(&bytes)?;
        let requested = bytes.len();
        let pattern_preview = hex_preview(pattern);

        if applied == 0 {
            self.set_warning_status(format!(
                "fill produced no bytes [pattern {pattern_preview}] (cursor at EOF; overwrite truncates)"
            ));
        } else if applied < requested {
            self.set_warning_status(format!(
                "filled {applied}/{requested} bytes [pattern {pattern_preview}] (truncated at EOF)"
            ));
        } else {
            self.set_info_status(format!(
                "filled {applied} bytes [pattern {pattern_preview}]"
            ));
        }

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

    fn execute_export_command(&mut self, format: ExportFormat) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return Err(HxError::MissingSelection);
        };

        let display_span = end - start + 1;
        let bytes = self.document.logical_bytes(start, end)?;

        match format {
            ExportFormat::Binary { path } => {
                std::fs::write(&path, &bytes)?;
                if display_span as usize != bytes.len() {
                    self.set_info_status(format!(
                        "exported {} logical bytes (display span {}) to {}",
                        bytes.len(),
                        display_span,
                        path.display()
                    ));
                } else {
                    self.set_info_status(format!(
                        "exported {} bytes to {}",
                        bytes.len(),
                        path.display()
                    ));
                }
            }
            ExportFormat::CArray { name } => {
                let ident = crate::export::sanitize_identifier(&name);
                let text = crate::export::format_c_array(&ident, &bytes);
                if crate::clipboard::copy_text(&text).is_ok() {
                    self.set_info_status(format!(
                        "exported {} bytes as C array '{}' [copied]",
                        bytes.len(),
                        ident
                    ));
                } else {
                    self.set_warning_status(format!(
                        "exported {} bytes as C array '{}' (clipboard unavailable)",
                        bytes.len(),
                        ident
                    ));
                }
            }
            ExportFormat::PythonBytes { name } => {
                let ident = crate::export::sanitize_identifier(&name);
                let text = crate::export::format_python_bytes(&ident, &bytes);
                if crate::clipboard::copy_text(&text).is_ok() {
                    self.set_info_status(format!(
                        "exported {} bytes as Python bytes '{}' [copied]",
                        bytes.len(),
                        ident
                    ));
                } else {
                    self.set_warning_status(format!(
                        "exported {} bytes as Python bytes '{}' (clipboard unavailable)",
                        bytes.len(),
                        ident
                    ));
                }
            }
        }

        Ok(())
    }

    fn execute_replace_command(
        &mut self,
        needle: &[u8],
        replacement: &[u8],
        allow_resize: bool,
    ) -> HxResult<()> {
        if needle.is_empty() {
            return Err(HxError::InvalidReplace(
                "needle must not be empty".to_owned(),
            ));
        }
        if !allow_resize && needle.len() != replacement.len() {
            return Err(HxError::InvalidReplace(
                "equal-length replace requires same-size needle/replacement; use :re! to resize"
                    .to_owned(),
            ));
        }
        if self.document.is_empty() {
            self.set_info_status("replace: no matches");
            return Ok(());
        }

        let active_selection = self.selection_range();
        let (start, end) = active_selection.unwrap_or((0, self.document.len() - 1));
        let matches = self.collect_replace_matches(start, end, needle)?;
        if matches.is_empty() {
            self.set_info_status("replace: no matches");
            return Ok(());
        }

        let cursor_before = self.cursor;
        let mode_before = self.mode;
        let cursor_after = matches[0];
        let outcome = if allow_resize {
            self.apply_replace_resizing(&matches, needle, replacement)?
        } else {
            self.apply_replace_same_size(&matches, replacement)?
        };

        if active_selection.is_some() {
            self.selection_anchor = None;
            self.mode = Mode::Normal;
        }
        let mode_after = if matches!(self.mode, Mode::Command) {
            self.normalize_mode(self.command_return_mode.unwrap_or(Mode::Normal))
        } else {
            self.mode
        };
        let cursor_after = self.clamp_cursor_for_mode(cursor_after, mode_after);
        self.cursor = cursor_after;
        self.refresh_inspector();

        self.push_undo_step(
            outcome.ops,
            cursor_before,
            mode_before,
            cursor_after,
            mode_after,
        );

        if outcome.stats.changed_bytes == 0 {
            self.set_info_status(format!(
                "replace matched {} spans; bytes unchanged",
                outcome.stats.match_count
            ));
        } else if allow_resize {
            self.set_info_status(format!(
                "replaced {} matches; total {}→{} bytes",
                outcome.stats.match_count, outcome.stats.before_bytes, outcome.stats.after_bytes
            ));
        } else {
            self.set_info_status(format!(
                "replaced {} matches; total {} bytes",
                outcome.stats.match_count, outcome.stats.after_bytes
            ));
        }

        Ok(())
    }

    fn collect_replace_matches(
        &mut self,
        start: u64,
        end_inclusive: u64,
        needle: &[u8],
    ) -> HxResult<Vec<u64>> {
        if start > end_inclusive || needle.is_empty() {
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();
        let mut search_start = start;
        let end = end_inclusive.min(self.document.len().saturating_sub(1));

        while search_start <= end {
            let Some(found) = self.document.search_forward(search_start, needle)? else {
                break;
            };
            let found_end = found + needle.len() as u64 - 1;
            if found > end || found_end > end {
                break;
            }

            matches.push(found);
            search_start = found.saturating_add(needle.len() as u64);
        }

        Ok(matches)
    }

    fn apply_replace_same_size(
        &mut self,
        matches: &[u64],
        replacement: &[u8],
    ) -> HxResult<ReplaceOutcome> {
        let mut changes = Vec::new();

        for &offset in matches {
            let ids = self
                .document
                .cell_ids_range(offset, replacement.len() as u64);
            for (id, &byte) in ids.into_iter().zip(replacement.iter()) {
                if self.document.is_tombstone(id) {
                    return Err(HxError::OffsetOutOfRange);
                }
                let before = self.document.replacement_state(id);
                self.document.replace_display_byte_by_id(id, byte)?;
                let after = self.document.replacement_state(id);
                if after != before {
                    changes.push(ReplacementChange { id, before, after });
                }
            }
        }

        Ok(ReplaceOutcome {
            ops: if changes.is_empty() {
                Vec::new()
            } else {
                vec![EditOp::ReplaceBytes {
                    changes: changes.clone(),
                }]
            },
            stats: ReplaceStats {
                match_count: matches.len(),
                before_bytes: matches.len() * replacement.len(),
                after_bytes: matches.len() * replacement.len(),
                changed_bytes: changes.len(),
            },
        })
    }

    fn apply_replace_resizing(
        &mut self,
        matches: &[u64],
        needle: &[u8],
        replacement: &[u8],
    ) -> HxResult<ReplaceOutcome> {
        let mut ops = Vec::new();

        for &offset in matches.iter().rev() {
            let removed = self
                .document
                .delete_range_real(offset, needle.len() as u64)?;
            if !removed.is_empty() {
                ops.push(EditOp::RealDelete {
                    offset,
                    cells: removed,
                });
            }

            let inserted = self.document.insert_bytes(offset, replacement)?;
            if !inserted.is_empty() {
                ops.push(EditOp::Insert {
                    offset,
                    cells: inserted,
                });
            }
        }

        Ok(ReplaceOutcome {
            ops,
            stats: ReplaceStats {
                match_count: matches.len(),
                before_bytes: matches.len() * needle.len(),
                after_bytes: matches.len() * replacement.len(),
                changed_bytes: matches.len() * needle.len().max(replacement.len()),
            },
        })
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

fn repeated_pattern(pattern: &[u8], len: usize) -> Vec<u8> {
    pattern.iter().copied().cycle().take(len).collect()
}

fn hex_preview(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
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
