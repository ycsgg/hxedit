use crate::app::{App, EditOp, PasteSource, PasteState};
use crate::clipboard;
use crate::copy::{format_selection, CopyDisplay, CopyFormat};
use crate::error::{HxError, HxResult};

impl PasteState {
    fn new(
        source: PasteSource,
        original_len: usize,
        used_len: usize,
        applied_len: usize,
        preview: bool,
        insert: bool,
        data: &[u8],
    ) -> Self {
        let head = paste_head(data);
        let action = if preview { "preview" } else { "paste" };
        let len_label = if original_len == used_len {
            used_len.to_string()
        } else {
            format!("{used_len}/{original_len}")
        };
        let effect = paste_effect_summary(insert, used_len, applied_len);
        Self {
            summary: format!("{action} {} {len_label}b [{head}] {effect}", source.label()),
        }
    }
}

fn paste_head(data: &[u8]) -> String {
    if data.is_empty() {
        return "--".to_owned();
    }

    let head = data
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if data.len() > 6 {
        format!("{head} ...")
    } else {
        head
    }
}

fn paste_effect_summary(insert: bool, requested_len: usize, applied_len: usize) -> String {
    if insert {
        format!("ins+{applied_len}")
    } else if applied_len < requested_len {
        format!("ovr{applied_len} drop{}", requested_len - applied_len)
    } else {
        format!("ovr{applied_len}")
    }
}

impl App {
    fn overwrite_paste_truncates(&self, requested: usize) -> bool {
        self.overwrite_applied_len(requested) < requested
    }

    fn overwrite_applied_len(&self, requested: usize) -> usize {
        requested.min(self.document.len().saturating_sub(self.cursor) as usize)
    }

    fn copy_status_label(format: CopyFormat, display: CopyDisplay) -> String {
        if matches!(display, CopyDisplay::Base64) {
            display.label().to_owned()
        } else {
            format!("{} {}", format.label(), display.label())
        }
    }

    fn paste_preview_status(
        &self,
        source: PasteSource,
        original_len: usize,
        used_len: usize,
        applied_len: usize,
        insert: bool,
        data: &[u8],
    ) -> String {
        let size = if original_len == used_len {
            format!("using {used_len} bytes")
        } else {
            format!("using {used_len} of {original_len} bytes")
        };
        let effect = if insert {
            format!("insert +{applied_len} bytes")
        } else if applied_len < used_len {
            format!(
                "overwrite {applied_len} bytes, drop {} at EOF",
                used_len - applied_len
            )
        } else {
            format!("overwrite {applied_len} bytes")
        };
        format!(
            "paste preview [{}; {}; head {}; {}]",
            source.label(),
            size,
            paste_head(data),
            effect
        )
    }

    pub(crate) fn copy_selection(
        &mut self,
        format: CopyFormat,
        display: CopyDisplay,
    ) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return Err(HxError::MissingSelection);
        };
        let display_span = end - start + 1;
        let bytes = self.document.logical_bytes(start, end)?;
        let text = format_selection(&bytes, format, display)?;
        clipboard::copy_text(&text)?;
        let label = Self::copy_status_label(format, display);
        if display_span as usize != bytes.len() {
            self.set_info_status(format!(
                "copied {} logical bytes (display span {}) [{}]",
                bytes.len(),
                display_span,
                label
            ));
        } else {
            self.set_info_status(format!("copied {} bytes [{}]", bytes.len(), label));
        }
        Ok(())
    }

    /// Read clipboard, decode, and paste using the given mode (overwrite or insert).
    pub(crate) fn paste_from_clipboard(
        &mut self,
        raw: bool,
        preview: bool,
        limit: Option<usize>,
        insert: bool,
    ) -> HxResult<()> {
        let (mut bytes, source) = if raw {
            (clipboard::read_raw_bytes()?, PasteSource::Raw)
        } else {
            let text = clipboard::read_text()?;
            let (bytes, source) = crate::util::parse::parse_paste_text(&text)?;
            (bytes, source.into())
        };
        let original_len = bytes.len();

        if let Some(limit) = limit {
            bytes.truncate(limit);
        }
        let used_len = bytes.len();
        let applied_len = if insert {
            used_len
        } else {
            self.overwrite_applied_len(used_len)
        };

        self.last_paste = Some(PasteState::new(
            source,
            original_len,
            used_len,
            applied_len,
            preview,
            insert,
            &bytes,
        ));
        let overwrite_truncated = !insert && self.overwrite_paste_truncates(used_len);

        if preview {
            let message = self.paste_preview_status(
                source,
                original_len,
                used_len,
                applied_len,
                insert,
                &bytes,
            );
            if overwrite_truncated {
                self.set_warning_status(message);
            } else {
                self.set_info_status(message);
            }
            return Ok(());
        }

        let mode_label = if insert { "insert-pasted" } else { "pasted" };

        if insert {
            let pasted = self.apply_paste_insert(&bytes)?;
            if pasted == 0 {
                self.set_info_status("paste produced no bytes");
            } else {
                self.set_info_status(format!(
                    "{mode_label} {} bytes [{}]",
                    pasted,
                    source.label()
                ));
            }
        } else {
            let pasted = self.apply_paste_overwrite(&bytes)?;
            if pasted == 0 {
                if overwrite_truncated {
                    self.set_warning_status(format!(
                        "paste produced no bytes [{}] (cursor at EOF; overwrite truncates)",
                        source.label()
                    ));
                } else {
                    self.set_info_status("paste produced no bytes");
                }
            } else if overwrite_truncated {
                self.set_warning_status(format!(
                    "{mode_label} {} bytes [{}] (truncated at EOF)",
                    pasted,
                    source.label()
                ));
            } else {
                self.set_info_status(format!(
                    "{mode_label} {} bytes [{}]",
                    pasted,
                    source.label()
                ));
            }
        }
        Ok(())
    }

    /// Overwrite-paste: replace existing bytes starting at cursor.
    /// Bytes that extend past EOF are silently dropped.
    pub(crate) fn apply_paste_overwrite(&mut self, bytes: &[u8]) -> HxResult<usize> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        if bytes.is_empty() {
            return Ok(0);
        }

        let cursor_before = self.cursor;
        let doc_len = self.document.len();
        let applied = bytes
            .len()
            .min(doc_len.saturating_sub(cursor_before) as usize);
        let ids = self.document.cell_ids_range(cursor_before, applied as u64);
        let mut ops = Vec::with_capacity(applied);

        for (byte, id) in bytes[..applied].iter().copied().zip(ids.into_iter()) {
            if self.document.is_tombstone(id) {
                return Err(HxError::OffsetOutOfRange);
            }
            let previous = self.document.replacement_state(id);
            self.document.replace_display_byte_by_id(id, byte)?;
            let after = self.document.replacement_state(id);
            if after != previous {
                ops.push(crate::app::ReplacementChange {
                    id,
                    before: previous,
                    after,
                });
            }
        }

        let written = applied;
        let cursor_after =
            self.clamp_cursor_for_mode(cursor_before + written.saturating_sub(1) as u64, self.mode);

        if !ops.is_empty() {
            self.push_undo_step(
                vec![EditOp::ReplaceBytes { changes: ops }],
                cursor_before,
                self.mode,
                cursor_after,
                self.mode,
            );
        }
        self.cursor = cursor_after;
        self.refresh_inspector();
        Ok(written)
    }

    /// Insert-paste: insert bytes at cursor, shifting subsequent offsets right.
    pub(crate) fn apply_paste_insert(&mut self, bytes: &[u8]) -> HxResult<usize> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        if bytes.is_empty() {
            return Ok(0);
        }

        let cursor_before = self.cursor;
        let inserted = self.document.insert_bytes(cursor_before, bytes)?;
        let cursor_after = self.clamp_cursor_for_mode(
            cursor_before + bytes.len().saturating_sub(1) as u64,
            self.mode,
        );
        self.push_undo_step(
            vec![EditOp::Insert {
                offset: cursor_before,
                cells: inserted,
            }],
            cursor_before,
            self.mode,
            cursor_after,
            self.mode,
        );
        self.cursor = cursor_after;
        self.refresh_inspector();
        Ok(bytes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_state_summary_mentions_source_lengths_and_effect() {
        let state = PasteState::new(
            PasteSource::Base64,
            12,
            8,
            3,
            true,
            false,
            &[0xde, 0xad, 0xbe, 0xef, 0x11, 0x22],
        );

        assert!(state.summary.contains("preview"));
        assert!(state.summary.contains("base64"));
        assert!(state.summary.contains("8/12b"));
        assert!(state.summary.contains("ovr3 drop5"));
        assert!(state.summary.contains("de ad be ef"));
    }

    #[test]
    fn base64_copy_status_label_omits_unused_group_format() {
        assert_eq!(
            App::copy_status_label(CopyFormat::QuadByte, CopyDisplay::Base64),
            "b64"
        );
    }
}
