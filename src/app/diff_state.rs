use std::path::PathBuf;

use crate::app::{App, SidePanelKind};
use crate::core::file_view::FileView;
use crate::diff::DiffOptions;
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

/// Runtime state for the synchronized diff page.
///
/// This state intentionally does not cache full-file hunks: opening `:diff`
/// must be cheap for large files. Rendering reads only the visible other-file
/// bytes and compares them with the current document's visible logical bytes.
#[derive(Debug)]
pub(crate) struct DiffState {
    pub other_path: PathBuf,
    pub options: DiffOptions,
    pub other_view: FileView,
    pub other_len: u64,
    pub revision_at_open: u64,
    pub stale: bool,
    /// Other-side raw byte selected from an aligned `OnlyOther` diff cell.
    /// The main editor cursor cannot point at that byte because it has no
    /// current-document display slot, so we keep the raw offset separately and
    /// render it active only while the cursor is still on its display anchor.
    pub selected_other_offset: Option<u64>,
    pub selected_other_anchor_display: Option<u64>,
}

impl App {
    pub(crate) fn diff_state(&self) -> Option<&DiffState> {
        self.diff_state.as_ref()
    }

    pub(crate) fn diff_state_mut(&mut self) -> Option<&mut DiffState> {
        self.diff_state.as_mut()
    }

    pub(crate) fn open_diff_panel(
        &mut self,
        other_path: PathBuf,
        max_shift: Option<usize>,
    ) -> HxResult<()> {
        let mut options = DiffOptions::default();
        if let Some(max_shift) = max_shift {
            options.max_shift = max_shift;
        }
        options = options.normalized();
        let other_view = FileView::open(
            &other_path,
            true,
            self.config.page_size,
            self.config.cache_pages,
        )?;
        let other_len = other_view.len();
        self.diff_state = Some(DiffState {
            other_path: other_path.clone(),
            options,
            other_view,
            other_len,
            revision_at_open: self.document_revision,
            stale: false,
            selected_other_offset: None,
            selected_other_anchor_display: None,
        });
        self.show_side_panel = true;
        self.active_side_panel = SidePanelKind::Diff;
        self.mode = Mode::SidePanel;
        self.set_info_status(format!(
            "diff page current logical bytes vs {} [synced; other 0x{:x} bytes]",
            other_path.display(),
            other_len
        ));
        Ok(())
    }

    pub(crate) fn refresh_diff_panel(&mut self) -> HxResult<()> {
        let (path, max_shift) = self
            .diff_state()
            .map(|state| (state.other_path.clone(), state.options.max_shift))
            .ok_or_else(|| HxError::CommandError("diff panel is not open".to_owned()))?;
        self.open_diff_panel(path, Some(max_shift))
    }

    pub(crate) fn close_diff_panel(&mut self) {
        self.diff_state = None;
        self.restore_inspector_after_side_panel_close();
        if self.inspector().is_some() || self.inspector_error.is_some() {
            self.show_side_panel = true;
            if self.mode.is_side_panel() {
                self.mode = Mode::SidePanel;
            }
            self.set_info_status("diff off");
        } else {
            self.show_side_panel = false;
            if self.mode.is_side_panel() {
                self.mode = Mode::Normal;
            }
            self.set_info_status("diff off (no format detected)");
        }
    }

    pub(crate) fn diff_projection_active(&self) -> bool {
        self.show_side_panel
            && self.active_side_panel == SidePanelKind::Diff
            && self.diff_state().is_some()
    }

    pub(crate) fn clear_diff_cell_selection(&mut self) {
        if let Some(state) = self.diff_state.as_mut() {
            state.selected_other_offset = None;
            state.selected_other_anchor_display = None;
        }
    }

    pub(crate) fn select_diff_other_cell(&mut self, other_offset: u64, anchor_display: u64) {
        if let Some(state) = self.diff_state.as_mut() {
            state.selected_other_offset = Some(other_offset);
            state.selected_other_anchor_display = Some(anchor_display);
        }
    }

    pub(crate) fn move_diff_selection(&mut self, delta: i64) {
        self.move_vertical(delta);
    }

    pub(crate) fn scroll_diff_panel(&mut self, rows: i64) {
        self.clear_diff_cell_selection();
        self.scroll_viewport(rows);
        self.sync_inspector_to_cursor();
        self.refresh_data_panel();
    }

    pub(crate) fn ensure_diff_selection_visible(&mut self) {}

    pub(crate) fn select_diff_panel_row(&mut self, visible_row: usize) {
        self.clear_diff_cell_selection();
        let target = self
            .viewport_top
            .saturating_add(visible_row as u64 * self.config.bytes_per_line as u64);
        self.cursor = self.clamp_offset(target);
        self.ensure_cursor_visible();
        self.sync_inspector_to_cursor();
        self.refresh_data_panel();
    }

    pub(crate) fn navigate_to_selected_diff_hunk(&mut self) -> HxResult<()> {
        self.clear_diff_cell_selection();
        self.jump_to_diff_mismatch(true)
    }

    pub(crate) fn jump_to_next_diff_mismatch(&mut self) -> HxResult<()> {
        self.jump_to_diff_mismatch(true)
    }

    pub(crate) fn jump_to_prev_diff_mismatch(&mut self) -> HxResult<()> {
        self.jump_to_diff_mismatch(false)
    }

    pub(crate) fn read_diff_other_byte(&mut self, offset: u64) -> HxResult<Option<u8>> {
        let Some(state) = self.diff_state_mut() else {
            return Ok(None);
        };
        if offset >= state.other_len {
            return Ok(None);
        }
        let bytes = state.other_view.read_range(offset, 1)?;
        Ok(bytes.first().copied())
    }

    pub(crate) fn mark_document_changed(&mut self) {
        self.document_revision = self.document_revision.saturating_add(1);
        self.mark_diff_stale();
    }

    pub(crate) fn mark_diff_stale(&mut self) {
        if let Some(state) = self.diff_state.as_mut() {
            // The page view compares visible bytes live, so current-document
            // edits do not require an expensive rescan. Keep the revision for
            // diagnostics but do not block live coloring behind refresh.
            state.revision_at_open = self.document_revision;
            state.stale = false;
        }
    }

    fn jump_to_diff_mismatch(&mut self, forward: bool) -> HxResult<()> {
        if self.diff_state().is_none() {
            return Err(HxError::CommandError("diff panel is not open".to_owned()));
        }
        if self.document.is_empty() {
            self.set_info_status("diff: no current bytes");
            return Ok(());
        }

        let start = if forward {
            self.cursor.saturating_add(1)
        } else {
            self.cursor.saturating_sub(1)
        };
        let found = if forward {
            match self.find_diff_mismatch_forward(start)? {
                Some(offset) => Some(offset),
                None => self.find_diff_mismatch_forward(0)?,
            }
        } else {
            match self.find_diff_mismatch_backward(start)? {
                Some(offset) => Some(offset),
                None => self
                    .document
                    .len()
                    .checked_sub(1)
                    .map_or(Ok(None), |end| self.find_diff_mismatch_backward(end))?,
            }
        };

        let Some(target) = found else {
            self.set_info_status("diff: no differing current-side bytes");
            return Ok(());
        };
        self.cursor = target;
        self.clear_diff_cell_selection();
        self.center_cursor_in_view();
        self.sync_inspector_to_cursor();
        self.refresh_data_panel();
        self.set_info_status(format!("diff mismatch @ display 0x{target:x}"));
        Ok(())
    }

    fn find_diff_mismatch_forward(&mut self, start: u64) -> HxResult<Option<u64>> {
        let mut offset = start.min(self.document.len());
        while offset < self.document.len() {
            if self.display_offset_is_diff_mismatch(offset)? {
                return Ok(Some(offset));
            }
            offset += 1;
        }
        Ok(None)
    }

    fn find_diff_mismatch_backward(&mut self, start: u64) -> HxResult<Option<u64>> {
        if self.document.is_empty() {
            return Ok(None);
        }
        let mut offset = start.min(self.document.len() - 1);
        loop {
            if self.display_offset_is_diff_mismatch(offset)? {
                return Ok(Some(offset));
            }
            if offset == 0 {
                break;
            }
            offset -= 1;
        }
        Ok(None)
    }

    fn display_offset_is_diff_mismatch(&mut self, display_offset: u64) -> HxResult<bool> {
        let current = match self.document.byte_at(display_offset)? {
            crate::core::document::ByteSlot::Present(byte) => byte,
            crate::core::document::ByteSlot::Deleted | crate::core::document::ByteSlot::Empty => {
                return Ok(false);
            }
        };
        let Some(logical) = self
            .document
            .logical_offset_for_display_offset(display_offset)
        else {
            return Ok(false);
        };
        Ok(self.read_diff_other_byte(logical)? != Some(current))
    }
}
