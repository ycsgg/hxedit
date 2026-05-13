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

    pub(super) fn execute_xor_command(&mut self, key: u8, in_place: bool) -> HxResult<()> {
        let Some((start, end)) = self.active_selection_range() else {
            return Err(HxError::MissingSelection);
        };

        let display_span = end - start + 1;
        let mut bytes = self.document.logical_bytes(start, end)?;
        if bytes.is_empty() {
            self.set_info_status("xor: no logical bytes in selection");
            return Ok(());
        }
        for byte in &mut bytes {
            *byte ^= key;
        }

        if in_place {
            self.apply_xor_in_place(start, end, key, &bytes)
        } else {
            self.copy_xor_result(key, display_span, &bytes)
        }
    }

    fn copy_xor_result(&mut self, key: u8, display_span: u64, bytes: &[u8]) -> HxResult<()> {
        let text = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        crate::clipboard::copy_text(&text)?;

        if display_span as usize != bytes.len() {
            self.set_info_status(format!(
                "xor 0x{key:02x}: copied {} logical bytes (display span {}) [hex]",
                bytes.len(),
                display_span
            ));
        } else {
            self.set_info_status(format!(
                "xor 0x{key:02x}: copied {} bytes [hex]",
                bytes.len()
            ));
        }
        Ok(())
    }

    fn apply_xor_in_place(
        &mut self,
        start: u64,
        end: u64,
        key: u8,
        xored_bytes: &[u8],
    ) -> HxResult<()> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        if xored_bytes.is_empty() {
            self.set_info_status("xor!: no logical bytes in selection");
            return Ok(());
        }

        let cursor_before = self.cursor;
        let mode_before = self.mode;
        let span = end - start + 1;
        let ids = self.document.cell_ids_range(start, span);
        let mut changes = Vec::with_capacity(xored_bytes.len());
        let mut xored = xored_bytes.iter().copied();

        for id in ids {
            if self.document.is_tombstone(id) {
                continue;
            }
            let Some(byte) = xored.next() else {
                break;
            };
            let before = self.document.replacement_state(id);
            self.document.replace_display_byte_by_id(id, byte)?;
            let after = self.document.replacement_state(id);
            if after != before {
                changes.push(ReplacementChange { id, before, after });
            }
        }

        debug_assert!(xored.next().is_none());

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

        let changed_count = changes.len();

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
            self.push_undo_step(
                vec![EditOp::ReplaceBytes { changes }],
                cursor_before,
                mode_before,
                cursor_after,
                mode_after,
            );
        }

        if changed_count == 0 {
            self.set_info_status(format!(
                "xor! 0x{key:02x}: {} logical bytes unchanged",
                xored_bytes.len()
            ));
        } else {
            self.set_info_status(format!(
                "xor! 0x{key:02x}: replaced {} logical bytes in place",
                xored_bytes.len()
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
