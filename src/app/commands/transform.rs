use std::io::Write;

use super::*;

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
    pub(super) fn execute_fill_command(&mut self, pattern: &[u8], len: usize) -> HxResult<()> {
        if pattern.is_empty() || len == 0 {
            self.set_info_status("fill produced no bytes");
            return Ok(());
        }

        let applied = self.apply_fill_overwrite(pattern, len as u64)?;
        let requested = len;
        let pattern_preview = hex_preview(pattern);

        if applied == 0 {
            self.set_warning_status(format!(
                "fill produced no bytes [pattern {pattern_preview}] (cursor at EOF; overwrite truncates)"
            ));
        } else if (applied as usize) < requested {
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

    /// Overwrite-fill `run_len` display cells from the cursor with a repeating
    /// pattern, streaming so neither the repeated pattern nor the full cell-id
    /// list is materialized up front. Mirrors `apply_paste_overwrite`'s undo,
    /// cursor, and inspector behavior.
    fn apply_fill_overwrite(&mut self, pattern: &[u8], run_len: u64) -> HxResult<u64> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        let cursor_before = self.cursor;
        let pattern_len = pattern.len() as u64;
        let doc_len = self.document.len();
        let applied = if cursor_before >= doc_len {
            0
        } else {
            run_len.min(doc_len - cursor_before)
        };
        let use_bulk_undo = self
            .document
            .replacement_range_is_pristine(cursor_before, applied);

        let (written, changed_count, ops) = if use_bulk_undo {
            let stats = self.document.overwrite_run_positional_compact(
                cursor_before,
                run_len,
                |run_index| pattern[(run_index % pattern_len) as usize],
            )?;
            let ops = if stats.changed == 0 {
                Vec::new()
            } else {
                vec![EditOp::ReplaceBulk {
                    offset: cursor_before,
                    len: stats.visited,
                    before: BulkReplacement::Clear,
                    after: BulkReplacement::Pattern(pattern.to_vec()),
                }]
            };
            (stats.visited, stats.changed as usize, ops)
        } else {
            let (written, changes) =
                self.document
                    .overwrite_run_positional(cursor_before, run_len, |run_index| {
                        pattern[(run_index % pattern_len) as usize]
                    })?;
            let changes: Vec<ReplacementChange> = changes
                .into_iter()
                .map(|(id, before, after)| ReplacementChange { id, before, after })
                .collect();
            let changed_count = changes.len();
            let ops = if changes.is_empty() {
                Vec::new()
            } else {
                vec![EditOp::ReplaceBytes { changes }]
            };
            (written, changed_count, ops)
        };

        if written == 0 {
            return Ok(0);
        }

        let cursor_after =
            self.clamp_cursor_for_mode(cursor_before + written.saturating_sub(1), self.mode);
        if changed_count > 0 {
            self.push_undo_step(ops, cursor_before, self.mode, cursor_after, self.mode);
        }
        self.cursor = cursor_after;
        self.invalidate_disassembly_cache();
        self.refresh_inspector();
        Ok(written)
    }

    pub(super) fn execute_paste_command(
        &mut self,
        raw: bool,
        preview: bool,
        limit: Option<usize>,
        insert: bool,
    ) -> HxResult<()> {
        self.paste_from_clipboard(raw, preview, limit, insert)
    }

    pub(super) fn execute_export_command(&mut self, format: ExportFormat) -> HxResult<()> {
        let Some((start, end)) = self.active_selection_range() else {
            return Err(HxError::MissingSelection);
        };

        let display_span = end - start + 1;

        // Binary export streams logical bytes to disk in 64 KB chunks so a
        // multi-GB selection never materializes the whole range in memory.
        if let ExportFormat::Binary { path } = &format {
            let file = std::fs::File::create(path)?;
            let mut writer = std::io::BufWriter::new(file);
            let written =
                self.document
                    .for_each_logical_chunk(start, end, |chunk| -> HxResult<()> {
                        writer.write_all(chunk)?;
                        Ok(())
                    })?;
            writer.flush()?;
            let written = written as usize;
            if display_span as usize != written {
                self.set_info_status(format!(
                    "exported {} logical bytes (display span {}) to {}",
                    written,
                    display_span,
                    path.display()
                ));
            } else {
                self.set_info_status(format!("exported {} bytes to {}", written, path.display()));
            }
            return Ok(());
        }

        // C array / Python bytes still materialize the selection: they emit a
        // full text literal that is clipboard-bound anyway.
        let bytes = self.document.logical_bytes(start, end)?;

        match format {
            ExportFormat::Binary { .. } => unreachable!("binary export handled above"),
            ExportFormat::CArray { name } => {
                let ident = crate::export::sanitize_identifier(&name, &self.config.export_name);
                let text =
                    crate::export::format_c_array(&ident, &bytes, self.config.export_c_width);
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
                let ident = crate::export::sanitize_identifier(&name, &self.config.export_name);
                let text =
                    crate::export::format_python_bytes(&ident, &bytes, self.config.export_py_width);
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

    pub(super) fn execute_xor_command(&mut self, key: u8, in_place: bool) -> HxResult<()> {
        let Some((start, end)) = self.active_selection_range() else {
            return Err(HxError::MissingSelection);
        };

        if in_place {
            self.apply_xor_in_place(start, end, key)
        } else {
            self.copy_xor_result(start, end, key)
        }
    }

    fn copy_xor_result(&mut self, start: u64, end: u64, key: u8) -> HxResult<()> {
        // The clipboard payload is a full hex string anyway, but stream the
        // selection through 64 KB chunks so we never hold the raw logical bytes
        // and the formatted text at the same time.
        let display_span = end - start + 1;
        let mut text = String::new();
        let mut logical_len = 0u64;
        self.document
            .for_each_logical_chunk(start, end, |chunk| -> HxResult<()> {
                for &byte in chunk {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&format!("{:02x}", byte ^ key));
                }
                logical_len += chunk.len() as u64;
                Ok(())
            })?;

        if logical_len == 0 {
            self.set_info_status("xor: no logical bytes in selection");
            return Ok(());
        }

        crate::clipboard::copy_text(&text)?;

        if display_span != logical_len {
            self.set_info_status(format!(
                "xor 0x{key:02x}: copied {logical_len} logical bytes (display span {display_span}) [hex]"
            ));
        } else {
            self.set_info_status(format!("xor 0x{key:02x}: copied {logical_len} bytes [hex]"));
        }
        Ok(())
    }

    fn apply_xor_in_place(&mut self, start: u64, end: u64, key: u8) -> HxResult<()> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }

        let cursor_before = self.cursor;
        let mode_before = self.mode;
        // Stream the selection in 64 KB chunks: read → xor → write back as a
        // replacement, never materializing the whole logical range.
        let range_len = end.saturating_sub(start).saturating_add(1);
        let use_bulk_undo = key != 0
            && self
                .document
                .replacement_range_is_pristine(start, range_len);
        let (visible_count, changed_count, ops) = if use_bulk_undo {
            let stats =
                self.document
                    .transform_visible_range_in_place_compact(start, end, |byte| byte ^ key)?;
            let ops = if stats.changed == 0 {
                Vec::new()
            } else {
                vec![EditOp::ReplaceBulk {
                    offset: start,
                    len: stats.visited,
                    before: BulkReplacement::Clear,
                    after: BulkReplacement::Xor { key },
                }]
            };
            (stats.visited, stats.changed as usize, ops)
        } else {
            let (visible_count, raw_changes) =
                self.document
                    .transform_visible_range_in_place(start, end, |byte| byte ^ key)?;
            let changes: Vec<ReplacementChange> = raw_changes
                .into_iter()
                .map(|(id, before, after)| ReplacementChange { id, before, after })
                .collect();
            let changed_count = changes.len();
            let ops = if changes.is_empty() {
                Vec::new()
            } else {
                vec![EditOp::ReplaceBytes { changes }]
            };
            (visible_count, changed_count, ops)
        };
        if visible_count == 0 {
            self.set_info_status("xor!: no logical bytes in selection");
            return Ok(());
        }
        let visual_selection = self.selection_range();
        let inspector_selection = visual_selection.is_none()
            && self.active_side_panel == SidePanelKind::Inspector
            && (self.mode.is_side_panel()
                || self
                    .command_return_mode
                    .is_some_and(|mode| mode.is_side_panel()));
        if visual_selection.is_some() {
            self.selection_anchor = None;
            self.mode = Mode::Normal;
        }
        let mode_after = if matches!(self.mode, Mode::Command) {
            self.normalize_mode(self.command_return_mode.unwrap_or(Mode::Normal))
        } else {
            self.mode
        };
        let mode_after = if inspector_selection && matches!(mode_after, Mode::Normal) {
            Mode::SidePanel
        } else {
            mode_after
        };
        let cursor_after = self.clamp_cursor_for_mode(start, mode_after);
        self.cursor = cursor_after;

        if changed_count > 0 {
            self.invalidate_disassembly_cache();
        }
        self.refresh_inspector();
        if inspector_selection && self.inspector().is_some() {
            self.mode = mode_after;
            self.sync_cursor_to_inspector();
        }
        let cursor_after = self.cursor;
        if changed_count > 0 {
            self.push_undo_step(ops, cursor_before, mode_before, cursor_after, mode_after);
        }

        if changed_count == 0 {
            self.set_info_status(format!(
                "xor! 0x{key:02x}: {visible_count} logical bytes unchanged"
            ));
        } else {
            self.set_info_status(format!(
                "xor! 0x{key:02x}: replaced {visible_count} logical bytes in place"
            ));
        }
        Ok(())
    }

    pub(super) fn execute_replace_command(
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
        if matches!(self.main_view, crate::app::MainView::Disassembly(_)) && allow_resize {
            return Err(HxError::DisassemblyUnavailable(
                "view is overwrite-only; use :re without ! for equal-length replace".to_owned(),
            ));
        }
        if self.document.is_empty() {
            self.set_info_status("replace: no matches");
            return Ok(());
        }

        let visual_selection = self.selection_range();
        let active_selection = self.active_selection_range();
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

        if visual_selection.is_some() {
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
        if !outcome.ops.is_empty() {
            self.invalidate_disassembly_cache();
        }
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
}

fn hex_preview(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
