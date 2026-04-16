use crate::app::{App, EditOp, PasteSource, PasteState};
use crate::clipboard;
use crate::copy::{format_selection, CopyDisplay, CopyFormat};
use crate::error::{HxError, HxResult};

impl App {
    pub(crate) fn copy_selection(
        &mut self,
        format: CopyFormat,
        display: CopyDisplay,
    ) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return Err(HxError::MissingSelection);
        };
        let bytes = self.document.logical_bytes(start, end)?;
        let text = format_selection(&bytes, format, display)?;
        clipboard::copy_text(&text)?;
        self.status_message = format!(
            "copied {} bytes [{} {}]",
            bytes.len(),
            format.label(),
            display.label()
        );
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

        if let Some(limit) = limit {
            bytes.truncate(limit);
        }

        self.last_paste = Some(PasteState {
            summary: crate::app::helpers::paste_summary(source, bytes.len(), preview, &bytes),
        });

        if preview {
            self.status_message = if bytes.is_empty() {
                "paste preview: no bytes".to_owned()
            } else {
                format!("paste preview [{} {} bytes]", source.label(), bytes.len())
            };
            return Ok(());
        }

        let mode_label = if insert { "insert-pasted" } else { "pasted" };

        if insert {
            let pasted = self.apply_paste_insert(&bytes)?;
            if pasted == 0 {
                self.status_message = "paste produced no bytes".to_owned();
            } else {
                self.status_message = format!("{mode_label} {} bytes [{}]", pasted, source.label());
            }
        } else {
            let pasted = self.apply_paste_overwrite(&bytes)?;
            if pasted == 0 {
                self.status_message = "paste produced no bytes".to_owned();
            } else {
                self.status_message = format!("{mode_label} {} bytes [{}]", pasted, source.label());
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
        let mut ops = Vec::new();

        for (i, &byte) in bytes.iter().enumerate() {
            let offset = cursor_before + i as u64;
            if offset >= doc_len {
                break; // past EOF — stop overwriting
            }
            let id = self
                .document
                .cell_id_at(offset)
                .ok_or(HxError::OffsetOutOfRange)?;
            let previous = self.document.replacement_state(id);
            self.document.replace_display_byte(offset, byte)?;
            let after = self.document.replacement_state(id);
            if after != previous {
                ops.push(crate::app::ReplacementUndo { id, previous });
            }
        }

        let written = bytes.len().min((doc_len - cursor_before) as usize);

        if !ops.is_empty() {
            self.push_undo_step(
                vec![EditOp::ReplaceBytes { changes: ops }],
                cursor_before,
                self.mode,
            );
        }
        self.cursor =
            self.clamp_cursor_for_mode(cursor_before + written.saturating_sub(1) as u64, self.mode);
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
        self.document.insert_bytes(cursor_before, bytes)?;
        self.push_undo_step(
            vec![EditOp::Insert {
                offset: cursor_before,
                len: bytes.len() as u64,
            }],
            cursor_before,
            self.mode,
        );
        self.cursor = self.clamp_cursor_for_mode(
            cursor_before + bytes.len().saturating_sub(1) as u64,
            self.mode,
        );
        self.refresh_inspector();
        Ok(bytes.len())
    }
}
