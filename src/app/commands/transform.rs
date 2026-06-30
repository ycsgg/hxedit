use super::*;

impl App {
    pub(super) fn execute_fill_command(&mut self, pattern: &[u8], len: usize) -> HxResult<()> {
        if pattern.is_empty() || len == 0 {
            self.set_info_status("fill produced no bytes");
            return Ok(());
        }

        let cursor_before = self.cursor;
        let result =
            crate::exec::fill_overwrite(&mut self.document, cursor_before, pattern, len as u64)?;
        let applied = result.written;
        let cursor_after =
            self.clamp_cursor_for_mode(cursor_before + applied.saturating_sub(1), self.mode);
        if !result.ops.is_empty() {
            self.push_undo_step(
                result.ops,
                cursor_before,
                self.mode,
                cursor_after,
                self.mode,
            );
        }
        if applied > 0 {
            self.cursor = cursor_after;
        }
        if result.changed > 0 {
            self.invalidate_disassembly_cache();
        }
        self.refresh_inspector();
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
            let export = crate::exec::export_binary_range(&mut self.document, start, end, path)?;
            if display_span != export.bytes_written {
                self.set_info_status(format!(
                    "exported {} logical bytes (display span {}) to {}",
                    export.bytes_written,
                    display_span,
                    path.display()
                ));
            } else {
                self.set_info_status(format!(
                    "exported {} bytes to {}",
                    export.bytes_written,
                    path.display()
                ));
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
        let result = crate::exec::xor_in_place(&mut self.document, start, end, key)?;
        let visible_count = result.visited;
        let changed_count = result.changed;
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
            self.push_undo_step(
                result.ops,
                cursor_before,
                mode_before,
                cursor_after,
                mode_after,
            );
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
        force: bool,
    ) -> HxResult<()> {
        if matches!(self.main_view, crate::app::MainView::Disassembly(_))
            && allow_resize
            && needle.len() != replacement.len()
        {
            return Err(HxError::DisassemblyUnavailable(
                "view is overwrite-only; use :re without ! for equal-length replace".to_owned(),
            ));
        }

        let visual_selection = self.selection_range();
        let active_selection = self.active_selection_range();
        let (start, end) = if let Some(range) = active_selection {
            range
        } else if self.document.is_empty() {
            (0, 0)
        } else {
            (0, self.document.len() - 1)
        };

        let cursor_before = self.cursor;
        let mode_before = self.mode;
        let outcome = match crate::exec::replace_range(
            &mut self.document,
            start,
            end,
            needle,
            replacement,
            allow_resize,
            force,
        )? {
            crate::exec::ReplaceResult::Applied(outcome) => outcome,
            crate::exec::ReplaceResult::NoMatches => {
                self.set_info_status("replace: no matches");
                return Ok(());
            }
            crate::exec::ReplaceResult::TooManyMatches { limit } => {
                self.set_warning_status(format!(
                    "replace found more than {limit} matches; rerun with --force to apply"
                ));
                return Ok(());
            }
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
        let cursor_after = self.clamp_cursor_for_mode(outcome.first_match, mode_after);
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
        } else if outcome.stats.before_bytes != outcome.stats.after_bytes {
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
}

fn hex_preview(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
