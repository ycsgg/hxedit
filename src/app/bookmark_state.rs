use crate::app::{App, SidePanelKind};
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookmarkColor {
    Default,
    Red,
    Yellow,
    Green,
    Blue,
    Magenta,
    Cyan,
}

impl BookmarkColor {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Red => "red",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Magenta => "magenta",
            Self::Cyan => "cyan",
        }
    }

    pub(crate) const fn palette_index(self) -> usize {
        match self {
            Self::Default => 0,
            Self::Red => 1,
            Self::Yellow => 2,
            Self::Green => 3,
            Self::Blue => 4,
            Self::Magenta => 5,
            Self::Cyan => 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BookmarkEntry {
    pub id: u64,
    pub name: String,
    pub start: u64,
    pub len: u64,
    pub color: BookmarkColor,
    pub note: Option<String>,
    pub created_revision: u64,
}

impl BookmarkEntry {
    pub(crate) fn end(&self) -> u64 {
        self.start.saturating_add(self.len.saturating_sub(1))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BookmarkState {
    pub entries: Vec<BookmarkEntry>,
    pub selected_row: usize,
    pub scroll_offset: usize,
    pub detail_scroll_offset: usize,
    pub next_id: u64,
}

impl Default for BookmarkState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            selected_row: 0,
            scroll_offset: 0,
            detail_scroll_offset: 0,
            next_id: 1,
        }
    }
}

impl BookmarkState {
    pub(crate) fn add(
        &mut self,
        name: String,
        start: u64,
        len: u64,
        color: BookmarkColor,
        note: Option<String>,
        revision: u64,
    ) -> HxResult<u64> {
        if len == 0 {
            return Err(HxError::CommandError(
                "bookmark length must be greater than zero".to_owned(),
            ));
        }
        start
            .checked_add(len - 1)
            .ok_or(HxError::OffsetOutOfRange)?;
        if name.is_empty() || name.chars().any(char::is_whitespace) || name.starts_with('#') {
            return Err(HxError::CommandError(
                "bookmark name must be one non-empty token and must not start with #".to_owned(),
            ));
        }
        if self.entries.iter().any(|entry| entry.name == name) {
            return Err(HxError::CommandError(format!(
                "bookmark {name} already exists"
            )));
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| HxError::CommandError("bookmark id space is exhausted".to_owned()))?;
        self.entries.push(BookmarkEntry {
            id,
            name,
            start,
            len,
            color,
            note,
            created_revision: revision,
        });
        self.entries.sort_by_key(|entry| (entry.start, entry.id));
        if let Some(index) = self.entries.iter().position(|entry| entry.id == id) {
            self.selected_row = index;
            self.detail_scroll_offset = 0;
        }
        Ok(id)
    }

    pub(crate) fn remove(&mut self, selector: &str) -> HxResult<BookmarkEntry> {
        let index = self
            .find_index(selector)
            .ok_or_else(|| HxError::CommandError(format!("bookmark not found: {selector}")))?;
        Ok(self.remove_index(index))
    }

    pub(crate) fn remove_selected(&mut self) -> HxResult<BookmarkEntry> {
        if self.entries.is_empty() {
            return Err(HxError::CommandError("no bookmark selected".to_owned()));
        }
        Ok(self.remove_index(self.selected_row))
    }

    fn remove_index(&mut self, index: usize) -> BookmarkEntry {
        let removed = self.entries.remove(index);
        self.selected_row = self.selected_row.min(self.entries.len().saturating_sub(1));
        self.scroll_offset = self.scroll_offset.min(self.entries.len().saturating_sub(1));
        self.detail_scroll_offset = 0;
        removed
    }

    pub(crate) fn find_index(&self, selector: &str) -> Option<usize> {
        if let Some(id) = selector
            .strip_prefix('#')
            .and_then(|value| value.parse::<u64>().ok())
        {
            self.entries.iter().position(|entry| entry.id == id)
        } else {
            self.entries.iter().position(|entry| entry.name == selector)
        }
    }

    pub(crate) fn selected_entry(&self) -> Option<&BookmarkEntry> {
        self.entries.get(self.selected_row)
    }

    pub(crate) fn set_note(&mut self, selector: &str, note: Option<String>) -> HxResult<()> {
        let index = self
            .find_index(selector)
            .ok_or_else(|| HxError::CommandError(format!("bookmark not found: {selector}")))?;
        self.entries[index].note = note;
        self.selected_row = index;
        self.detail_scroll_offset = 0;
        Ok(())
    }

    pub(crate) fn next_from(&self, cursor: u64) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.start > cursor)
            .or_else(|| (!self.entries.is_empty()).then_some(0))
    }

    pub(crate) fn prev_from(&self, cursor: u64) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|entry| entry.start < cursor)
            .or_else(|| (!self.entries.is_empty()).then_some(self.entries.len() - 1))
    }
}

impl App {
    pub(crate) fn bookmark_state(&self) -> &BookmarkState {
        &self.bookmark_state
    }

    pub(crate) fn bookmark_state_mut(&mut self) -> &mut BookmarkState {
        &mut self.bookmark_state
    }

    pub(crate) fn focus_bookmark_panel(&mut self) {
        self.close_diff_projection_for_side_panel_switch();
        self.active_side_panel = SidePanelKind::Bookmarks;
        self.show_side_panel = true;
        self.mode = Mode::SidePanel;
        self.ensure_bookmark_selection_visible();
    }

    pub(crate) fn move_bookmark_selection(&mut self, delta: i64) {
        let count = self.bookmark_state.entries.len();
        if count == 0 {
            return;
        }
        let state = self.bookmark_state_mut();
        let next = if delta >= 0 {
            state
                .selected_row
                .saturating_add(delta as usize)
                .min(count - 1)
        } else {
            state
                .selected_row
                .saturating_sub(delta.unsigned_abs() as usize)
        };
        state.selected_row = next;
        state.detail_scroll_offset = 0;
        self.ensure_bookmark_selection_visible();
    }

    pub(crate) fn ensure_bookmark_selection_visible(&mut self) {
        let visible_rows = self.bookmark_list_visible_rows();
        let state = self.bookmark_state_mut();
        if state.selected_row < state.scroll_offset {
            state.scroll_offset = state.selected_row;
        } else if state.selected_row >= state.scroll_offset.saturating_add(visible_rows) {
            state.scroll_offset = state.selected_row.saturating_sub(visible_rows - 1);
        }
    }

    pub(crate) fn bookmark_list_visible_rows(&self) -> usize {
        crate::view::bookmark_panel::list_height(self.view_rows as u16)
    }

    pub(crate) fn move_bookmark_selection_to_edge(&mut self, end: bool) {
        let Some(last) = self.bookmark_state.entries.len().checked_sub(1) else {
            return;
        };
        self.bookmark_state.selected_row = if end { last } else { 0 };
        self.bookmark_state.detail_scroll_offset = 0;
        self.ensure_bookmark_selection_visible();
    }

    pub(crate) fn scroll_bookmark_detail(&mut self, delta: i64) {
        if self.bookmark_state.entries.is_empty() {
            return;
        }
        let width = self
            .last_columns
            .and_then(|columns| columns.side_panel.map(|area| area.width))
            .unwrap_or(32);
        let total = crate::view::bookmark_panel::detail_lines(
            &self.bookmark_state,
            width,
            self.document_revision,
            &self.palette,
        )
        .len();
        let visible = self
            .view_rows
            .saturating_sub(self.bookmark_list_visible_rows())
            .saturating_sub(1)
            .max(1);
        let max_offset = total.saturating_sub(visible);
        let current = self.bookmark_state.detail_scroll_offset;
        self.bookmark_state.detail_scroll_offset = if delta >= 0 {
            current.saturating_add(delta as usize).min(max_offset)
        } else {
            current.saturating_sub(delta.unsigned_abs() as usize)
        };
    }

    pub(crate) fn delete_selected_bookmark(&mut self) -> HxResult<()> {
        let removed = self.bookmark_state.remove_selected()?;
        self.ensure_bookmark_selection_visible();
        self.set_info_status(format!("deleted bookmark {}", removed.name));
        Ok(())
    }

    pub(crate) fn navigate_to_selected_bookmark(&mut self) -> HxResult<()> {
        let entry = self
            .bookmark_state
            .selected_entry()
            .cloned()
            .ok_or_else(|| HxError::CommandError("no bookmark selected".to_owned()))?;
        self.goto_bookmark_entry(&entry)
    }

    pub(crate) fn handle_bookmark_panel_click(&mut self, visible_row: usize) {
        if visible_row >= self.bookmark_list_visible_rows() {
            return;
        }
        let actual_row = self
            .bookmark_state
            .scroll_offset
            .saturating_add(visible_row);
        if actual_row >= self.bookmark_state.entries.len() {
            return;
        }
        self.bookmark_state.selected_row = actual_row;
        self.bookmark_state.detail_scroll_offset = 0;
        if let Err(err) = self.navigate_to_selected_bookmark() {
            self.set_error_status(err.to_string());
        }
    }

    pub(crate) fn goto_bookmark_entry(&mut self, entry: &BookmarkEntry) -> HxResult<()> {
        self.cursor = self.document.goto(entry.start)?;
        self.ensure_cursor_visible();
        self.set_info_status(format!(
            "bookmark {}: display 0x{:x}",
            entry.name, entry.start
        ));
        Ok(())
    }
}
