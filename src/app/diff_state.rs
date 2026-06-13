use std::path::PathBuf;

use crate::app::{App, SidePanelKind};
use crate::core::file_view::FileView;
use crate::diff::{find_mismatch_backward_step, find_mismatch_forward_step, DiffOptions};
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

const DIFF_MISMATCH_STEP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffMismatchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffMismatchPhase {
    Primary,
    Wrapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffMismatchScan {
    direction: DiffMismatchDirection,
    phase: DiffMismatchPhase,
    origin: u64,
    range_start: u64,
    range_end: u64,
    cursor: u64,
    scanned: u64,
}

impl DiffMismatchScan {
    fn new(direction: DiffMismatchDirection, origin: u64, doc_len: u64) -> Option<Self> {
        if doc_len == 0 {
            return None;
        }
        let last = doc_len - 1;
        let (range_start, range_end, cursor, phase) = match direction {
            DiffMismatchDirection::Forward => {
                let start = origin.saturating_add(1);
                if start <= last {
                    (start, last, start, DiffMismatchPhase::Primary)
                } else {
                    (0, last, 0, DiffMismatchPhase::Wrapped)
                }
            }
            DiffMismatchDirection::Backward => {
                if origin > 0 {
                    (0, origin - 1, origin - 1, DiffMismatchPhase::Primary)
                } else {
                    (0, 0, 0, DiffMismatchPhase::Primary)
                }
            }
        };
        Some(Self {
            direction,
            phase,
            origin: origin.min(last),
            range_start,
            range_end,
            cursor,
            scanned: 0,
        })
    }

    fn advance_to_wrap(&mut self, doc_len: u64) -> bool {
        if self.phase == DiffMismatchPhase::Wrapped || doc_len == 0 {
            return false;
        }
        let last = doc_len - 1;
        self.phase = DiffMismatchPhase::Wrapped;
        match self.direction {
            DiffMismatchDirection::Forward => {
                self.range_start = 0;
                self.range_end = self.origin.min(last);
                self.cursor = self.range_start;
            }
            DiffMismatchDirection::Backward => {
                self.range_start = self.origin.min(last);
                self.range_end = last;
                self.cursor = self.range_end;
            }
        }
        self.range_start <= self.range_end
    }

    fn direction_label(self) -> &'static str {
        match self.direction {
            DiffMismatchDirection::Forward => "next",
            DiffMismatchDirection::Backward => "previous",
        }
    }
}

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
    pub pending_mismatch_scan: Option<DiffMismatchScan>,
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
            pending_mismatch_scan: None,
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

    pub(crate) fn diff_mismatch_scan_pending(&self) -> bool {
        self.diff_state()
            .and_then(|state| state.pending_mismatch_scan)
            .is_some()
    }

    pub(crate) fn cancel_diff_mismatch_scan(&mut self, message: Option<&str>) {
        if let Some(state) = self.diff_state.as_mut() {
            state.pending_mismatch_scan = None;
        }
        if let Some(message) = message {
            self.set_info_status(message);
        }
    }

    pub(crate) fn report_diff_mismatch_scan_blocked_input(&mut self) {
        if let Some(scan) = self
            .diff_state()
            .and_then(|state| state.pending_mismatch_scan)
        {
            self.set_diff_mismatch_scan_status(scan);
        }
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
            state.pending_mismatch_scan = None;
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

        let direction = if forward {
            DiffMismatchDirection::Forward
        } else {
            DiffMismatchDirection::Backward
        };

        let Some(scan) = DiffMismatchScan::new(direction, self.cursor, self.document.len()) else {
            self.set_info_status("diff: no current bytes");
            return Ok(());
        };
        if let Some(state) = self.diff_state.as_mut() {
            state.pending_mismatch_scan = Some(scan);
        }
        self.clear_diff_cell_selection();
        self.set_diff_mismatch_scan_status(scan);
        self.continue_diff_mismatch_scan()
    }

    pub(crate) fn continue_diff_mismatch_scan(&mut self) -> HxResult<()> {
        let Some(mut scan) = self
            .diff_state
            .as_mut()
            .and_then(|state| state.pending_mismatch_scan.take())
        else {
            return Ok(());
        };

        if self.document.is_empty() {
            self.set_info_status("diff: no current bytes");
            return Ok(());
        }

        let step = {
            let Some(state) = self.diff_state.as_mut() else {
                return Ok(());
            };
            match scan.direction {
                DiffMismatchDirection::Forward => find_mismatch_forward_step(
                    &mut self.document,
                    &mut state.other_view,
                    state.other_len,
                    scan.cursor,
                    scan.range_end,
                    DIFF_MISMATCH_STEP_BYTES,
                )?,
                DiffMismatchDirection::Backward => find_mismatch_backward_step(
                    &mut self.document,
                    &mut state.other_view,
                    state.other_len,
                    scan.range_start,
                    scan.cursor,
                    DIFF_MISMATCH_STEP_BYTES,
                )?,
            }
        };

        scan.scanned = scan.scanned.saturating_add(step.scanned);
        if let Some(target) = step.found {
            self.finish_diff_mismatch_scan(target);
            return Ok(());
        }
        if let Some(next) = step.next {
            scan.cursor = next;
            if let Some(state) = self.diff_state.as_mut() {
                state.pending_mismatch_scan = Some(scan);
            }
            self.set_diff_mismatch_scan_status(scan);
            return Ok(());
        }
        if scan.advance_to_wrap(self.document.len()) {
            if let Some(state) = self.diff_state.as_mut() {
                state.pending_mismatch_scan = Some(scan);
            }
            self.set_diff_mismatch_scan_status(scan);
            return Ok(());
        }

        self.set_info_status("diff: no differing current-side bytes");
        Ok(())
    }

    fn finish_diff_mismatch_scan(&mut self, target: u64) {
        if let Some(state) = self.diff_state.as_mut() {
            state.pending_mismatch_scan = None;
        }
        self.cursor = target;
        self.clear_diff_cell_selection();
        self.center_cursor_in_view();
        self.sync_inspector_to_cursor();
        self.refresh_data_panel();
        self.set_info_status(format!("diff mismatch @ display 0x{target:x}"));
    }

    fn set_diff_mismatch_scan_status(&mut self, scan: DiffMismatchScan) {
        self.set_info_status(format!(
            "diff scanning {} mismatch... {} checked; Esc to cancel",
            scan.direction_label(),
            format_scan_bytes(scan.scanned)
        ));
    }
}

fn format_scan_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} bytes")
    }
}
