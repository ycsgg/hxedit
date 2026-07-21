use super::*;
use crate::app::BookmarkColor;
use crate::commands::types::{BookmarkColorArg, BookmarkCommand};

impl App {
    pub(super) fn execute_bookmark_command(&mut self, command: BookmarkCommand) -> HxResult<()> {
        match command {
            BookmarkCommand::Panel => {
                self.focus_bookmark_panel();
                self.set_info_status(format!(
                    "bookmark view ({} marks)",
                    self.bookmark_state.entries.len()
                ));
                Ok(())
            }
            BookmarkCommand::Add {
                name,
                start,
                len,
                color,
                note,
            } => self.add_bookmark_command(name, start, len, color.into(), note),
            BookmarkCommand::Note { selector, note } => {
                let cleared = note.is_none();
                self.bookmark_state.set_note(&selector, note)?;
                self.ensure_bookmark_selection_visible();
                let action = if cleared { "cleared" } else { "updated" };
                self.set_info_status(format!("{action} bookmark {selector} note"));
                Ok(())
            }
            BookmarkCommand::Delete { selector } => {
                let removed = self.bookmark_state.remove(&selector)?;
                self.ensure_bookmark_selection_visible();
                self.set_info_status(format!("deleted bookmark {}", removed.name));
                Ok(())
            }
            BookmarkCommand::Clear => {
                let count = self.bookmark_state.entries.len();
                self.bookmark_state = Default::default();
                self.set_info_status(format!("cleared {count} bookmarks"));
                Ok(())
            }
            BookmarkCommand::Goto { selector } => {
                let index = self.bookmark_state.find_index(&selector).ok_or_else(|| {
                    HxError::CommandError(format!("bookmark not found: {selector}"))
                })?;
                self.bookmark_state.selected_row = index;
                self.bookmark_state.detail_scroll_offset = 0;
                self.ensure_bookmark_selection_visible();
                let entry = self.bookmark_state.entries[index].clone();
                self.goto_bookmark_entry(&entry)
            }
            BookmarkCommand::Next => self.jump_bookmark_from_cursor(true),
            BookmarkCommand::Prev => self.jump_bookmark_from_cursor(false),
        }
    }

    fn add_bookmark_command(
        &mut self,
        name: Option<String>,
        start: Option<u64>,
        len: Option<u64>,
        color: BookmarkColor,
        note: Option<String>,
    ) -> HxResult<()> {
        let (start, len) = match (start, len) {
            (Some(start), len) => (start, len.unwrap_or(1)),
            (None, Some(_)) => {
                return Err(HxError::CommandError(
                    "bookmark length requires an explicit start".to_owned(),
                ));
            }
            (None, None) => self.default_bookmark_range()?,
        };
        if len == 0 {
            return Err(HxError::CommandError(
                "bookmark length must be greater than zero".to_owned(),
            ));
        }
        let end = start
            .checked_add(len - 1)
            .ok_or(HxError::OffsetOutOfRange)?;
        if self.document.is_empty() || end >= self.document.len() {
            return Err(HxError::OffsetOutOfRange);
        }
        let name = match name {
            Some(name) => name,
            None => self.next_default_bookmark_name()?,
        };
        self.bookmark_state.add(
            name.clone(),
            start,
            len,
            color,
            note.clone(),
            self.document_revision,
        )?;
        self.selection_anchor = None;
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        self.focus_bookmark_panel();
        let note_suffix = if note.is_some() { " with note" } else { "" };
        self.set_info_status(format!(
            "bookmark {name}: display 0x{start:x}..0x{end:x}{note_suffix}"
        ));
        Ok(())
    }

    fn next_default_bookmark_name(&self) -> HxResult<String> {
        let mut suffix = self.bookmark_state.next_id;
        loop {
            let candidate = format!("mark_{suffix}");
            if self
                .bookmark_state
                .entries
                .iter()
                .all(|entry| entry.name != candidate)
            {
                return Ok(candidate);
            }
            suffix = suffix.checked_add(1).ok_or_else(|| {
                HxError::CommandError("bookmark name space is exhausted".to_owned())
            })?;
        }
    }

    fn default_bookmark_range(&self) -> HxResult<(u64, u64)> {
        if let Some((start, end)) = self.active_selection_range() {
            return Ok((start, end - start + 1));
        }
        if self.document.is_empty() {
            return Err(HxError::OffsetOutOfRange);
        }
        Ok((self.cursor_anchor_offset(), 1))
    }

    fn jump_bookmark_from_cursor(&mut self, forward: bool) -> HxResult<()> {
        let index = if forward {
            self.bookmark_state.next_from(self.cursor_anchor_offset())
        } else {
            self.bookmark_state.prev_from(self.cursor_anchor_offset())
        }
        .ok_or_else(|| HxError::CommandError("no bookmarks".to_owned()))?;
        self.bookmark_state.selected_row = index;
        self.bookmark_state.detail_scroll_offset = 0;
        self.ensure_bookmark_selection_visible();
        let entry = self.bookmark_state.entries[index].clone();
        self.goto_bookmark_entry(&entry)
    }
}

impl From<BookmarkColorArg> for BookmarkColor {
    fn from(value: BookmarkColorArg) -> Self {
        match value {
            BookmarkColorArg::Default => Self::Default,
            BookmarkColorArg::Red => Self::Red,
            BookmarkColorArg::Yellow => Self::Yellow,
            BookmarkColorArg::Green => Self::Green,
            BookmarkColorArg::Blue => Self::Blue,
            BookmarkColorArg::Magenta => Self::Magenta,
            BookmarkColorArg::Cyan => Self::Cyan,
        }
    }
}
