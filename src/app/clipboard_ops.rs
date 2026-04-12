use crate::app::{App, PasteSource, PasteState, UndoEntry};
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

    pub(crate) fn paste_from_clipboard(
        &mut self,
        raw: bool,
        preview: bool,
        limit: Option<usize>,
    ) -> HxResult<()> {
        let (mut bytes, source) = if raw {
            (clipboard::read_raw_bytes()?, PasteSource::Raw)
        } else {
            let text = clipboard::read_text()?;
            crate::app::helpers::parse_paste_payload(&text)?
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

        let pasted = self.apply_paste_bytes(&bytes)?;
        if pasted == 0 {
            self.status_message = "paste produced no bytes".to_owned();
        } else {
            self.status_message = format!("pasted {} bytes [{}]", pasted, source.label());
        }
        Ok(())
    }

    pub(crate) fn apply_paste_bytes(&mut self, bytes: &[u8]) -> HxResult<usize> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        if bytes.is_empty() {
            return Ok(0);
        }

        let cursor_before = self.cursor;
        let mode_before = self.mode;
        let mut undo_entries = Vec::with_capacity(bytes.len());
        for (idx, &byte) in bytes.iter().enumerate() {
            let offset = cursor_before + idx as u64;
            let previous_patch = self.document.patch_state_at(offset);
            self.document.set_byte(offset, byte)?;
            if self.document.patch_state_at(offset) != previous_patch {
                undo_entries.push(UndoEntry {
                    offset,
                    previous_patch,
                    cursor_before,
                    mode_before,
                });
            }
        }

        self.push_undo_step(undo_entries);
        self.cursor = self.clamp_offset(cursor_before + bytes.len().saturating_sub(1) as u64);
        Ok(bytes.len())
    }
}
