use crate::app::{App, SidePanelKind, StatusLevel};
#[cfg(feature = "memory")]
use crate::app::{BookmarkState, UndoStep};
use crate::mode::Mode;

/// `(region_index, replacement spans)` pairs queued for `:mem commit-all`.
#[cfg(feature = "memory")]
type DirtyRegionSpans = Vec<(usize, Vec<(u64, Vec<u8>)>)>;

/// Move `current` by `delta` within `[0, count)` with saturating bounds.
#[cfg(feature = "memory")]
fn step_index(current: usize, delta: isize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    if delta < 0 {
        current.saturating_sub((-delta) as usize)
    } else {
        current.saturating_add(delta as usize).min(count - 1)
    }
}

/// Number of non-region lines rendered above the maps-view region list
/// ("Process memory" header, pid, state, base VA, blank). Mouse hit-testing
/// maps a body row to a region index by subtracting this and adding scroll.
#[cfg(feature = "memory")]
pub(crate) const MEMORY_MAPS_HEADER_ROWS: usize = 5;

/// Each region occupies two body rows in the maps view: one for the address /
/// permissions summary and one for the label / path.
#[cfg(feature = "memory")]
pub(crate) const MEMORY_MAPS_REGION_ROWS: usize = 2;

#[cfg(feature = "memory")]
pub(crate) struct MemoryRuntime {
    pub(crate) session: crate::memory::MemorySession,
    pub(crate) selected_region: usize,
    pub(crate) opened_region: usize,
    pub(crate) base_va: u64,
    /// Per-region editing state for regions that are not the currently opened
    /// document. Lets undo/redo stacks and pending replacements survive region
    /// switches so `:mem commit-all` and cross-region recovery work.
    pub(crate) region_edits: std::collections::HashMap<usize, RegionEditState>,
    /// Session bookmarks are display-relative to one memory-region document.
    /// Keep each region's annotations separate when the active document changes.
    pub(crate) region_bookmarks: std::collections::HashMap<usize, RegionBookmarkState>,
}

/// Editing state captured for a region while it is not the opened document.
#[cfg(feature = "memory")]
#[derive(Debug, Clone, Default)]
pub(crate) struct RegionEditState {
    pub(crate) spans: Vec<(u64, Vec<u8>)>,
    pub(crate) undo: Vec<UndoStep>,
    pub(crate) redo: Vec<UndoStep>,
    pub(crate) cursor: u64,
}

#[cfg(feature = "memory")]
#[derive(Debug, Clone)]
pub(crate) struct RegionBookmarkState {
    pub(crate) state: BookmarkState,
    pub(crate) revision_at_stash: u64,
}

#[cfg(feature = "memory")]
impl RegionEditState {
    pub(crate) fn dirty_bytes(&self) -> usize {
        self.spans.iter().map(|(_, bytes)| bytes.len()).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "memory"), allow(dead_code))]
pub(crate) enum MemoryPanelView {
    /// Process info + region maps for the active session.
    Maps,
    /// Process picker populated by `:mem list`.
    ProcessList,
    /// Aggregated `:mem info` report.
    Info,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryPanelState {
    pub(crate) view: MemoryPanelView,
    pub(crate) message: String,
    pub(crate) scroll_offset: usize,
    pub(crate) selected_row: usize,
    /// Cached process list for the `ProcessList` view (avoids re-enumerating
    /// every frame).
    #[cfg(feature = "memory")]
    pub(crate) processes: Vec<crate::memory::ProcessInfo>,
}

impl App {
    pub(crate) fn memory_state(&self) -> Option<&MemoryPanelState> {
        self.memory_state.as_ref()
    }

    pub(crate) fn open_memory_panel(&mut self, message: impl Into<String>) {
        self.close_diff_projection_for_side_panel_switch();
        let message = message.into();
        let selected_row = self
            .memory_state
            .as_ref()
            .map_or(0, |state| state.selected_row);
        #[cfg(feature = "memory")]
        let selected_row = self
            .memory_runtime
            .as_ref()
            .map_or(selected_row, |runtime| runtime.selected_region);
        self.memory_state = Some(MemoryPanelState {
            view: MemoryPanelView::Maps,
            message: message.clone(),
            scroll_offset: 0,
            selected_row,
            #[cfg(feature = "memory")]
            processes: Vec::new(),
        });
        self.active_side_panel = SidePanelKind::Memory;
        self.show_side_panel = true;
        self.mode = Mode::SidePanel;
        self.set_status(StatusLevel::Notice, message);
    }

    /// Open the panel in process-list mode with the enumerated processes.
    #[cfg(feature = "memory")]
    pub(crate) fn open_memory_process_list_panel(
        &mut self,
        processes: Vec<crate::memory::ProcessInfo>,
        message: impl Into<String>,
    ) {
        self.close_diff_projection_for_side_panel_switch();
        let message = message.into();
        self.memory_state = Some(MemoryPanelState {
            view: MemoryPanelView::ProcessList,
            message: message.clone(),
            scroll_offset: 0,
            selected_row: 0,
            processes,
        });
        self.active_side_panel = SidePanelKind::Memory;
        self.show_side_panel = true;
        self.mode = Mode::SidePanel;
        self.set_status(StatusLevel::Notice, message);
    }

    /// Open the panel in info mode showing the aggregated `:mem info` report.
    #[cfg(feature = "memory")]
    pub(crate) fn open_memory_info_panel(&mut self, text: impl Into<String>) {
        self.close_diff_projection_for_side_panel_switch();
        let text = text.into();
        let selected_row = self
            .memory_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.selected_region);
        self.memory_state = Some(MemoryPanelState {
            view: MemoryPanelView::Info,
            message: text.clone(),
            scroll_offset: 0,
            selected_row,
            processes: Vec::new(),
        });
        self.active_side_panel = SidePanelKind::Memory;
        self.show_side_panel = true;
        self.mode = Mode::SidePanel;
        self.set_status(StatusLevel::Notice, text.lines().next().unwrap_or_default());
    }

    #[cfg(feature = "memory")]
    pub(crate) fn memory_runtime(&self) -> Option<&MemoryRuntime> {
        self.memory_runtime.as_ref()
    }

    #[cfg(feature = "memory")]
    pub(crate) fn memory_runtime_mut(&mut self) -> Option<&mut MemoryRuntime> {
        self.memory_runtime.as_mut()
    }

    #[cfg(feature = "memory")]
    pub(crate) fn memory_base_va(&self) -> Option<u64> {
        self.memory_runtime
            .as_ref()
            .and_then(|runtime| self.document.is_fixed_size().then_some(runtime.base_va))
    }

    #[cfg(feature = "memory")]
    pub(crate) fn display_offset_to_va(&self, offset: u64) -> Option<u64> {
        self.memory_base_va()
            .and_then(|base| base.checked_add(offset))
    }

    #[cfg(not(feature = "memory"))]
    pub(crate) fn memory_base_va(&self) -> Option<u64> {
        None
    }

    #[cfg(not(feature = "memory"))]
    pub(crate) fn display_offset_to_va(&self, _offset: u64) -> Option<u64> {
        None
    }

    #[cfg(feature = "memory")]
    pub(crate) fn set_memory_runtime(&mut self, runtime: MemoryRuntime, message: String) {
        let selected_row = runtime.selected_region;
        self.memory_runtime = Some(runtime);
        self.close_diff_projection_for_side_panel_switch();
        self.memory_state = Some(MemoryPanelState {
            view: MemoryPanelView::Maps,
            message: message.clone(),
            scroll_offset: 0,
            selected_row,
            processes: Vec::new(),
        });
        self.active_side_panel = SidePanelKind::Memory;
        self.show_side_panel = true;
        self.mode = Mode::Normal;
        self.set_status(StatusLevel::Notice, message);
    }

    /// Attach to the process selected in the `ProcessList` view: open a new
    /// session and switch to the maps view. Blocked when the current session
    /// has uncommitted edits (dirty-switch guard).
    #[cfg(feature = "memory")]
    pub(crate) fn attach_selected_memory_process(&mut self) -> crate::error::HxResult<()> {
        let Some(pid) = self.memory_state.as_ref().and_then(|state| {
            state
                .processes
                .get(state.selected_row)
                .map(|process| process.pid)
        }) else {
            self.set_error_status("no process is selected");
            return Ok(());
        };
        if let Some((regions, bytes)) = self.memory_dirty_summary() {
            self.set_error_status(format!(
                "{regions} regions dirty, total {bytes} bytes; commit or :q! before switching process"
            ));
            return Ok(());
        }
        let config = self.config.clone();
        let backend = crate::memory::open_backend_for_pid(pid)?;
        let opened = Self::open_memory_cli_target(backend, &config)?;
        self.document = opened.document;
        self.cursor = 0;
        self.viewport_top = 0;
        self.selection_anchor = None;
        self.mouse_selection_anchor = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.bookmark_state = Default::default();
        self.document_revision = self.document_revision.saturating_add(1);
        self.invalidate_disassembly_cache();
        self.refresh_inspector();
        self.set_memory_runtime(opened.runtime, opened.message);
        Ok(())
    }

    #[cfg(feature = "memory")]
    pub(crate) fn move_memory_selection(&mut self, delta: isize) {
        let Some(view) = self.memory_state.as_ref().map(|state| state.view) else {
            return;
        };
        match view {
            MemoryPanelView::Maps => {
                let Some(runtime) = self.memory_runtime.as_ref() else {
                    return;
                };
                let count = runtime.session.regions().count();
                if count == 0 {
                    if let Some(runtime) = self.memory_runtime.as_mut() {
                        runtime.selected_region = 0;
                    }
                    return;
                }
                let current = runtime.selected_region.min(count - 1);
                let next = step_index(current, delta, count);
                if let Some(runtime) = self.memory_runtime.as_mut() {
                    runtime.selected_region = next;
                }
                self.sync_memory_panel_selection(next);
            }
            MemoryPanelView::ProcessList => {
                let count = self
                    .memory_state
                    .as_ref()
                    .map_or(0, |state| state.processes.len());
                if count == 0 {
                    return;
                }
                let current = self
                    .memory_state
                    .as_ref()
                    .map_or(0, |state| state.selected_row.min(count - 1));
                self.set_memory_selected_row(step_index(current, delta, count));
            }
            MemoryPanelView::Info => self.scroll_memory_panel(delta),
        }
    }

    /// Absolute selection set (used by mouse clicks). Clamps to the active
    /// view's row count and keeps the scroll offset following the selection.
    #[cfg(feature = "memory")]
    pub(crate) fn set_memory_selected_row(&mut self, row: usize) {
        let Some(view) = self.memory_state.as_ref().map(|state| state.view) else {
            return;
        };
        let count = match view {
            MemoryPanelView::Maps => self
                .memory_runtime
                .as_ref()
                .map_or(0, |runtime| runtime.session.regions().count()),
            MemoryPanelView::ProcessList => self
                .memory_state
                .as_ref()
                .map_or(0, |state| state.processes.len()),
            MemoryPanelView::Info => 0,
        };
        if count == 0 {
            return;
        }
        let row = row.min(count - 1);
        if view == MemoryPanelView::Maps {
            if let Some(runtime) = self.memory_runtime.as_mut() {
                runtime.selected_region = row;
            }
        }
        self.sync_memory_panel_selection(row);
    }

    /// Scroll the panel body without changing the selection (wheel / Info view).
    #[cfg(feature = "memory")]
    pub(crate) fn scroll_memory_panel(&mut self, delta: isize) {
        let Some((view, current_scroll)) = self
            .memory_state
            .as_ref()
            .map(|state| (state.view, state.scroll_offset))
        else {
            return;
        };
        let max_scroll = self.memory_panel_max_scroll(view);
        let next_scroll = if delta < 0 {
            current_scroll.saturating_sub((-delta) as usize)
        } else {
            current_scroll
                .saturating_add(delta as usize)
                .min(max_scroll)
        };
        if let Some(state) = self.memory_state.as_mut() {
            state.scroll_offset = next_scroll.min(max_scroll);
        }
    }

    /// Enter key in the memory panel: attach in process-list view, open the
    /// selected region in maps view, no-op in info view.
    #[cfg(feature = "memory")]
    pub(crate) fn handle_memory_panel_enter(&mut self) -> crate::error::HxResult<()> {
        match self.memory_state.as_ref().map(|state| state.view) {
            Some(MemoryPanelView::ProcessList) => self.attach_selected_memory_process(),
            Some(MemoryPanelView::Info) => Ok(()),
            _ => self.open_selected_memory_region(),
        }
    }

    /// Left-click on a memory panel body row at `visible_row` (0-based below the
    /// panel header). Maps a row to a region/process index and only changes the
    /// highlight — opening a region or attaching still requires Enter.
    #[cfg(feature = "memory")]
    pub(crate) fn handle_memory_panel_click(&mut self, visible_row: usize) {
        let Some((view, scroll)) = self.memory_state.as_ref().map(|state| {
            (
                state.view,
                state
                    .scroll_offset
                    .min(self.memory_panel_max_scroll(state.view)),
            )
        }) else {
            return;
        };
        let line = scroll + visible_row;
        match view {
            MemoryPanelView::Maps => {
                let Some(index) = self.memory_maps_region_index_for_line(line) else {
                    return;
                };
                let region = self
                    .memory_runtime
                    .as_ref()
                    .and_then(|runtime| runtime.session.region(index).cloned());
                let Some(region) = region else {
                    return;
                };
                self.set_memory_selected_row(index);
                if !region.permissions.read {
                    self.set_warning_status(format!(
                        "region 0x{:x}-0x{:x} is not readable",
                        region.start, region.end
                    ));
                }
            }
            MemoryPanelView::ProcessList => self.set_memory_selected_row(line),
            MemoryPanelView::Info => {}
        }
    }

    #[cfg(not(feature = "memory"))]
    pub(crate) fn handle_memory_panel_enter(&mut self) -> crate::error::HxResult<()> {
        self.open_selected_memory_region()
    }

    #[cfg(not(feature = "memory"))]
    pub(crate) fn handle_memory_panel_click(&mut self, _visible_row: usize) {}

    #[cfg(not(feature = "memory"))]
    pub(crate) fn scroll_memory_panel(&mut self, _delta: isize) {}

    #[cfg(not(feature = "memory"))]
    pub(crate) fn move_memory_selection(&mut self, _delta: isize) {}

    #[cfg(feature = "memory")]
    pub(crate) fn open_selected_memory_region(&mut self) -> crate::error::HxResult<()> {
        if self.memory_runtime.is_none() {
            self.open_memory_panel("memory region open requires an active memory session");
            return Ok(());
        }

        let index = self
            .memory_runtime
            .as_ref()
            .expect("checked above")
            .selected_region;
        let Some(region) = self
            .memory_runtime
            .as_ref()
            .expect("checked above")
            .session
            .region(index)
            .cloned()
        else {
            self.set_error_status("no memory region is selected");
            return Ok(());
        };
        self.open_memory_region_at(index, region.start)?;
        let message = format!(
            "opened memory region 0x{:x}-0x{:x} ({} bytes)",
            region.start,
            region.end,
            region.len()
        );
        if let Some(state) = self.memory_state.as_mut() {
            state.message = message.clone();
        }
        self.set_status(StatusLevel::Notice, message);
        Ok(())
    }

    #[cfg(feature = "memory")]
    pub(crate) fn execute_memory_search_command(
        &mut self,
        query: crate::memory::MemorySearchQuery,
        backward: bool,
    ) -> crate::error::HxResult<()> {
        if self.memory_runtime.is_none() {
            self.open_memory_panel("memory search requires an active memory session");
            return Ok(());
        }
        let start_addr = self.display_offset_to_va(self.cursor);
        let direction = if backward {
            crate::memory::MemorySearchDirection::Backward
        } else {
            crate::memory::MemorySearchDirection::Forward
        };
        let hit = {
            let runtime = self.memory_runtime.as_mut().expect("checked above");
            runtime.session.search(&query, start_addr, direction)?
        };
        // Remember the query so `gn` / `gN` can replay it, independent of the
        // file-search `/` `n` `p` history.
        self.last_memory_search = Some(query);
        let Some(hit) = hit else {
            self.set_info_status("memory search pattern not found");
            return Ok(());
        };

        self.open_memory_region_at(hit.region_index, hit.addr)?;
        let region = self
            .memory_runtime
            .as_ref()
            .and_then(|runtime| runtime.session.region(hit.region_index))
            .expect("hit region should still exist");
        let mut message = format!(
            "memory search hit at VA 0x{:x} in 0x{:x}-0x{:x}",
            hit.addr, region.start, region.end
        );
        if hit.wrapped {
            message.push_str(" [wrapped]");
        }
        if hit.skipped_regions > 0 {
            message.push_str(&format!(" [skipped {}]", hit.skipped_regions));
        }
        if let Some(state) = self.memory_state.as_mut() {
            state.message = message.clone();
        }
        if hit.wrapped {
            self.set_notice_status(message);
        } else {
            self.set_info_status(message);
        }
        Ok(())
    }

    /// Replay the last cross-region memory search (`gn` / `gN`). Uses the
    /// memory-search history, never the file-search `/` `n` `p` history.
    #[cfg(feature = "memory")]
    pub(crate) fn repeat_memory_search(&mut self, backward: bool) -> crate::error::HxResult<()> {
        if self.memory_runtime.is_none() {
            self.open_memory_panel("memory search requires an active memory session");
            return Ok(());
        }
        let Some(query) = self.last_memory_search.clone() else {
            self.set_info_status("no active memory search; use :ms first");
            return Ok(());
        };
        self.execute_memory_search_command(query, backward)
    }

    #[cfg(feature = "memory")]
    pub(crate) fn execute_memory_freeze_command(&mut self) -> crate::error::HxResult<()> {
        let Some(runtime) = self.memory_runtime.as_mut() else {
            self.open_memory_panel("memory freeze requires an active memory session");
            return Ok(());
        };
        runtime.session.freeze()?;
        let process = runtime.session.process_info();
        let depth = runtime.session.freeze_depth();
        let message = format!(
            "froze memory target {} ({}) [depth {depth}]",
            process.name, process.pid
        );
        if let Some(state) = self.memory_state.as_mut() {
            state.message = message.clone();
        }
        self.open_memory_panel(message);
        Ok(())
    }

    #[cfg(feature = "memory")]
    pub(crate) fn execute_memory_thaw_command(&mut self) -> crate::error::HxResult<()> {
        let Some(runtime) = self.memory_runtime.as_mut() else {
            self.open_memory_panel("memory thaw requires an active memory session");
            return Ok(());
        };
        runtime.session.thaw()?;
        let process = runtime.session.process_info();
        let message = if runtime.session.is_frozen() {
            format!(
                "decremented memory target freeze depth for {} ({}) [depth {}]",
                process.name,
                process.pid,
                runtime.session.freeze_depth()
            )
        } else {
            format!("thawed memory target {} ({})", process.name, process.pid)
        };
        if let Some(state) = self.memory_state.as_mut() {
            state.message = message.clone();
        }
        self.open_memory_panel(message);
        Ok(())
    }

    #[cfg(feature = "memory")]
    fn open_memory_region_at(
        &mut self,
        region_index: usize,
        addr: u64,
    ) -> crate::error::HxResult<()> {
        let config = self.config.clone();
        let region = self
            .memory_runtime
            .as_ref()
            .and_then(|runtime| runtime.session.region(region_index))
            .cloned()
            .ok_or(crate::error::HxError::OffsetOutOfRange)?;
        if self
            .memory_runtime
            .as_ref()
            .is_some_and(|runtime| runtime.opened_region == region_index)
            && self.document.is_fixed_size()
        {
            if let Some(runtime) = self.memory_runtime.as_mut() {
                runtime.selected_region = region_index;
                runtime.base_va = region.start;
            }
            self.cursor = self.clamp_cursor_for_mode(addr.saturating_sub(region.start), self.mode);
            self.viewport_top =
                super::navigation::align_offset(self.cursor, self.config.bytes_per_line);
            self.selection_anchor = None;
            self.mouse_selection_anchor = None;
            self.sync_memory_panel_selection(region_index);
            return Ok(());
        }
        // Switching to a different region: stash the editing state of the
        // region we are leaving so its undo/redo and pending replacements
        // survive, then restore the target region's saved state if any.
        self.stash_opened_region_edits();
        let saved = self
            .memory_runtime
            .as_mut()
            .and_then(|runtime| runtime.region_edits.remove(&region_index));
        let document = {
            let runtime = self.memory_runtime.as_mut().expect("checked above");
            runtime.session.document_for_region(region_index, &config)?
        };
        self.stash_opened_region_bookmarks();
        let restored_revision = self.document_revision.saturating_add(1);
        let bookmarks = {
            let runtime = self.memory_runtime.as_mut().expect("checked above");
            runtime.selected_region = region_index;
            runtime.opened_region = region_index;
            runtime.base_va = region.start;
            runtime
                .region_bookmarks
                .remove(&region_index)
                .map(|saved| {
                    let mut state = saved.state;
                    for entry in &mut state.entries {
                        if entry.created_revision == saved.revision_at_stash {
                            entry.created_revision = restored_revision;
                        }
                    }
                    state
                })
                .unwrap_or_default()
        };
        self.document = document;
        self.bookmark_state = bookmarks;
        self.undo_stack.clear();
        self.redo_stack.clear();
        let restored_cursor = if let Some(saved) = saved {
            self.document.apply_replacement_spans(&saved.spans)?;
            self.undo_stack = saved.undo;
            self.redo_stack = saved.redo;
            Some(saved.cursor)
        } else {
            None
        };
        let cursor_offset = restored_cursor.unwrap_or_else(|| addr.saturating_sub(region.start));
        self.cursor = self.clamp_cursor_for_mode(cursor_offset, self.mode);
        self.viewport_top =
            super::navigation::align_offset(self.cursor, self.config.bytes_per_line);
        self.selection_anchor = None;
        self.mouse_selection_anchor = None;
        self.document_revision = self.document_revision.saturating_add(1);
        self.invalidate_disassembly_cache();
        self.refresh_inspector();
        self.sync_memory_panel_selection(region_index);
        Ok(())
    }

    /// Capture the opened region's current editing state into `region_edits`
    /// so it can be restored after switching away. Clears the entry when the
    /// region has no pending edits.
    #[cfg(feature = "memory")]
    fn stash_opened_region_edits(&mut self) {
        if !self.document.is_fixed_size() {
            return;
        }
        let Some(opened) = self.memory_runtime.as_ref().map(|r| r.opened_region) else {
            return;
        };
        let spans = match self.document.replacement_spans() {
            Ok(spans) => spans,
            Err(err) => {
                self.set_error_status(err.to_string());
                return;
            }
        };
        let undo = std::mem::take(&mut self.undo_stack);
        let redo = std::mem::take(&mut self.redo_stack);
        let cursor = self.cursor;
        if let Some(runtime) = self.memory_runtime.as_mut() {
            if spans.is_empty() && undo.is_empty() && redo.is_empty() {
                runtime.region_edits.remove(&opened);
            } else {
                runtime.region_edits.insert(
                    opened,
                    RegionEditState {
                        spans,
                        undo,
                        redo,
                        cursor,
                    },
                );
            }
        }
    }

    #[cfg(feature = "memory")]
    fn stash_opened_region_bookmarks(&mut self) {
        let Some(opened) = self.memory_runtime.as_ref().map(|r| r.opened_region) else {
            return;
        };
        let revision_at_stash = self.document_revision;
        let bookmarks = std::mem::take(&mut self.bookmark_state);
        if let Some(runtime) = self.memory_runtime.as_mut() {
            if bookmarks.entries.is_empty() {
                runtime.region_bookmarks.remove(&opened);
            } else {
                runtime.region_bookmarks.insert(
                    opened,
                    RegionBookmarkState {
                        state: bookmarks,
                        revision_at_stash,
                    },
                );
            }
        }
    }

    #[cfg(feature = "memory")]
    fn sync_memory_panel_selection(&mut self, region_index: usize) {
        let Some(view) = self.memory_state.as_ref().map(|state| state.view) else {
            return;
        };
        let visible_rows = self.side_panel_visible_rows();
        let selected_line = match view {
            MemoryPanelView::Maps => self.memory_maps_line_for_region(region_index),
            MemoryPanelView::ProcessList => region_index,
            MemoryPanelView::Info => 0,
        };
        let max_scroll = self.memory_panel_max_scroll(view);
        if let Some(state) = self.memory_state.as_mut() {
            state.selected_row = region_index;
            if selected_line < state.scroll_offset {
                state.scroll_offset = selected_line;
            } else if selected_line >= state.scroll_offset + visible_rows {
                state.scroll_offset = selected_line.saturating_sub(visible_rows - 1);
            }
            state.scroll_offset = state.scroll_offset.min(max_scroll);
        }
    }

    #[cfg(feature = "memory")]
    fn memory_panel_line_count(&self, view: MemoryPanelView) -> usize {
        match view {
            MemoryPanelView::Maps => self.memory_runtime.as_ref().map_or(1, |runtime| {
                MEMORY_MAPS_HEADER_ROWS
                    + runtime
                        .session
                        .regions()
                        .count()
                        .saturating_mul(MEMORY_MAPS_REGION_ROWS)
            }),
            MemoryPanelView::ProcessList => self
                .memory_state
                .as_ref()
                .map_or(0, |state| state.processes.len()),
            MemoryPanelView::Info => self
                .memory_state
                .as_ref()
                .map_or(0, |state| state.message.lines().count()),
        }
    }

    #[cfg(feature = "memory")]
    fn memory_panel_max_scroll(&self, view: MemoryPanelView) -> usize {
        self.memory_panel_line_count(view)
            .saturating_sub(self.side_panel_visible_rows())
    }

    #[cfg(feature = "memory")]
    fn memory_maps_line_for_region(&self, region_index: usize) -> usize {
        MEMORY_MAPS_HEADER_ROWS + region_index.saturating_mul(MEMORY_MAPS_REGION_ROWS)
    }

    #[cfg(feature = "memory")]
    fn memory_maps_region_index_for_line(&self, line: usize) -> Option<usize> {
        let relative = line.checked_sub(MEMORY_MAPS_HEADER_ROWS)?;
        let index = relative / MEMORY_MAPS_REGION_ROWS;
        let count = self
            .memory_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.session.regions().count());
        (index < count).then_some(index)
    }

    #[cfg(feature = "memory")]
    pub(crate) fn commit_memory_document(
        &mut self,
        commit_all: bool,
    ) -> crate::error::HxResult<()> {
        if commit_all {
            return self.commit_all_memory_regions();
        }
        if !self.document.is_fixed_size() {
            self.open_memory_panel("memory commit requires an active memory document");
            return Ok(());
        }
        let spans = self.document.replacement_spans()?;
        if spans.is_empty() {
            self.open_memory_panel("memory document has no pending replacements");
            return Ok(());
        }

        let Some(runtime) = self.memory_runtime.as_ref() else {
            self.open_memory_panel("memory commit requires an active memory session");
            return Ok(());
        };
        let region_index = runtime.opened_region;
        let Some(region) = runtime.session.region(region_index).cloned() else {
            self.set_error_status("no memory region is selected");
            return Ok(());
        };
        if !region.permissions.write {
            return Err(crate::error::HxError::MemoryAccess {
                addr: region.start,
                len: region.len() as usize,
                message: "region is not writable".to_owned(),
            });
        }

        let total_bytes = spans.iter().map(|(_, bytes)| bytes.len()).sum::<usize>();
        let target_was_running = !runtime.session.is_frozen();
        {
            let runtime = self.memory_runtime.as_mut().expect("checked above");
            for (offset, bytes) in &spans {
                let addr = region
                    .start
                    .checked_add(*offset)
                    .ok_or(crate::error::HxError::OffsetOutOfRange)?;
                runtime.session.write_at(addr, bytes)?;
            }
            runtime.session.clear_region_dirty(region_index)?;
            runtime.region_edits.remove(&region_index);
            runtime.base_va = region.start;
        }

        let config = self.config.clone();
        let document = self
            .memory_runtime
            .as_mut()
            .expect("checked above")
            .session
            .document_for_region(region_index, &config)?;
        self.document = document;
        self.document.clear_replacements();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.document_revision = self.document_revision.saturating_add(1);
        self.cursor = self.clamp_cursor_for_mode(self.cursor, self.mode);
        self.invalidate_disassembly_cache();
        self.refresh_inspector();

        let mut message = format!(
            "memory commit wrote {total_bytes} byte{} across {} span{} at 0x{:x}-0x{:x}",
            if total_bytes == 1 { "" } else { "s" },
            spans.len(),
            if spans.len() == 1 { "" } else { "s" },
            region.start,
            region.end
        );
        if target_was_running {
            message.push_str(" [warning: target was running; use :mem freeze for safer edits]");
        }
        if let Some(state) = self.memory_state.as_mut() {
            state.message = message.clone();
        }
        self.open_memory_panel(message);
        Ok(())
    }

    /// Walk every dirty region in the session in virtual-address order and
    /// commit its pending replacements, stopping at the first failure and
    /// reporting which regions were written. Per mem-design.md §7.2.
    #[cfg(feature = "memory")]
    fn commit_all_memory_regions(&mut self) -> crate::error::HxResult<()> {
        if self.memory_runtime.is_none() {
            self.open_memory_panel("memory commit requires an active memory session");
            return Ok(());
        }
        // Make the opened region's live document the authoritative source for
        // its own spans, then merge with the stashed per-region edits.
        let opened = self
            .memory_runtime
            .as_ref()
            .expect("checked above")
            .opened_region;
        let opened_spans = if self.document.is_fixed_size() {
            self.document.replacement_spans()?
        } else {
            Vec::new()
        };

        let mut dirty: DirtyRegionSpans = Vec::new();
        {
            let runtime = self.memory_runtime.as_ref().expect("checked above");
            for (index, edit) in &runtime.region_edits {
                if *index != opened && !edit.spans.is_empty() {
                    dirty.push((*index, edit.spans.clone()));
                }
            }
        }
        if !opened_spans.is_empty() {
            dirty.push((opened, opened_spans));
        }
        if dirty.is_empty() {
            self.open_memory_panel("memory session has no pending replacements");
            return Ok(());
        }
        {
            let runtime = self.memory_runtime.as_ref().expect("checked above");
            dirty.sort_by_key(|(index, _)| {
                runtime
                    .session
                    .region(*index)
                    .map_or(u64::MAX, |region| region.start)
            });
        }

        let total_regions = dirty.len();
        let target_was_running = !self
            .memory_runtime
            .as_ref()
            .expect("checked above")
            .session
            .is_frozen();
        let mut committed_regions = 0usize;
        let mut committed_bytes = 0usize;
        let mut opened_committed = false;

        for (index, spans) in &dirty {
            let Some(region) = self
                .memory_runtime
                .as_ref()
                .expect("checked above")
                .session
                .region(*index)
                .cloned()
            else {
                continue;
            };
            if !region.permissions.write {
                let message = format!(
                    "memory commit-all stopped at 0x{:x}-0x{:x}: region is not writable; {committed_regions}/{total_regions} regions committed ({committed_bytes} bytes), remaining left dirty",
                    region.start, region.end
                );
                self.finish_commit_all(opened_committed);
                self.set_error_status(message.clone());
                if let Some(state) = self.memory_state.as_mut() {
                    state.message = message;
                }
                return Ok(());
            }
            let mut region_bytes = 0usize;
            let runtime = self.memory_runtime.as_mut().expect("checked above");
            for (offset, bytes) in spans {
                let addr = match region.start.checked_add(*offset) {
                    Some(addr) => addr,
                    None => continue,
                };
                if let Err(err) = runtime.session.write_at(addr, bytes) {
                    let message = format!(
                        "memory commit-all stopped at VA 0x{addr:x}: {err}; {committed_regions}/{total_regions} regions committed ({committed_bytes} bytes), remaining left dirty"
                    );
                    self.finish_commit_all(opened_committed);
                    self.set_error_status(message.clone());
                    if let Some(state) = self.memory_state.as_mut() {
                        state.message = message;
                    }
                    return Ok(());
                }
                region_bytes += bytes.len();
            }
            runtime.session.clear_region_dirty(*index)?;
            if *index == opened {
                opened_committed = true;
            } else {
                runtime.region_edits.remove(index);
            }
            committed_regions += 1;
            committed_bytes += region_bytes;
        }

        self.finish_commit_all(opened_committed);
        let mut message = format!(
            "memory commit-all wrote {committed_bytes} byte{} across {committed_regions} region{}",
            if committed_bytes == 1 { "" } else { "s" },
            if committed_regions == 1 { "" } else { "s" }
        );
        if target_was_running {
            message.push_str(" [warning: target was running; use :mem freeze for safer edits]");
        }
        if let Some(state) = self.memory_state.as_mut() {
            state.message = message.clone();
        }
        self.open_memory_panel(message);
        Ok(())
    }

    /// Rebuild the opened region's document after a commit-all so the rendered
    /// view matches the bytes now held by the target.
    #[cfg(feature = "memory")]
    fn finish_commit_all(&mut self, opened_committed: bool) {
        if !opened_committed {
            return;
        }
        let Some(opened) = self.memory_runtime.as_ref().map(|r| r.opened_region) else {
            return;
        };
        let config = self.config.clone();
        let document = self
            .memory_runtime
            .as_mut()
            .and_then(|runtime| runtime.session.document_for_region(opened, &config).ok());
        if let Some(document) = document {
            self.document = document;
            self.document.clear_replacements();
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.document_revision = self.document_revision.saturating_add(1);
            self.cursor = self.clamp_cursor_for_mode(self.cursor, self.mode);
            self.invalidate_disassembly_cache();
            self.refresh_inspector();
        }
    }

    /// Total dirty regions and bytes across the whole session, combining the
    /// opened region's live document with stashed per-region edits. Used by the
    /// `:q` quit guard and `:mem info` aggregation.
    #[cfg(feature = "memory")]
    pub(crate) fn memory_dirty_summary(&self) -> Option<(usize, usize)> {
        let runtime = self.memory_runtime.as_ref()?;
        let opened = runtime.opened_region;
        let mut regions = 0usize;
        let mut bytes = 0usize;
        for (index, edit) in &runtime.region_edits {
            if *index == opened {
                continue;
            }
            let region_bytes = edit.dirty_bytes();
            if region_bytes > 0 {
                regions += 1;
                bytes += region_bytes;
            }
        }
        if self.document.is_fixed_size() {
            let opened_bytes = self.document.replacement_dirty_bytes();
            if opened_bytes > 0 {
                regions += 1;
                bytes += opened_bytes;
            }
        }
        if regions == 0 {
            None
        } else {
            Some((regions, bytes))
        }
    }

    /// Pending replacement bytes for a single region, sourcing the opened
    /// region from its live document and others from stashed `region_edits`.
    #[cfg(feature = "memory")]
    fn memory_region_dirty_bytes(&self, index: usize) -> usize {
        let Some(runtime) = self.memory_runtime.as_ref() else {
            return 0;
        };
        if index == runtime.opened_region && self.document.is_fixed_size() {
            self.document.replacement_dirty_bytes()
        } else {
            runtime
                .region_edits
                .get(&index)
                .map_or(0, RegionEditState::dirty_bytes)
        }
    }

    /// Aggregated `:mem info` report (mem-design.md §7.3): selected region and
    /// fingerprint, its dirty bytes and undo/redo depth, session-wide dirty
    /// region totals with stale-base flags, backend access mode, and freeze
    /// state. Returns one line per `\n`-joined fact.
    #[cfg(feature = "memory")]
    pub(crate) fn memory_info_text(&self) -> String {
        let Some(runtime) = self.memory_runtime.as_ref() else {
            return "memory info requires an active memory session".to_owned();
        };
        let selected = runtime.selected_region;
        let opened = runtime.opened_region;
        let mut lines: Vec<String> = Vec::new();

        if let Some(region) = runtime.session.region(selected) {
            let perms = region.permissions.label();
            lines.push(format!(
                "region 0x{:x}-0x{:x} {}{}{} {} bytes fp=0x{:x}",
                region.start,
                region.end,
                perms[0],
                perms[1],
                perms[2],
                region.len(),
                region.fingerprint.0
            ));
            lines.push(format!(
                "access {}",
                if region.permissions.write { "rw" } else { "ro" }
            ));
        } else {
            lines.push("no memory region is selected".to_owned());
        }

        let (undo_depth, redo_depth) = if selected == opened {
            (self.undo_stack.len(), self.redo_stack.len())
        } else {
            runtime
                .region_edits
                .get(&selected)
                .map_or((0, 0), |edit| (edit.undo.len(), edit.redo.len()))
        };
        lines.push(format!(
            "selected dirty {} bytes, undo {undo_depth} / redo {redo_depth}",
            self.memory_region_dirty_bytes(selected)
        ));

        let (dirty_regions, dirty_bytes) = self.memory_dirty_summary().unwrap_or((0, 0));
        lines.push(format!(
            "session dirty {dirty_regions} region{}, {dirty_bytes} bytes total",
            if dirty_regions == 1 { "" } else { "s" }
        ));

        let mut dirty_list: Vec<String> = Vec::new();
        for (index, region) in runtime.session.regions().enumerate() {
            let bytes = self.memory_region_dirty_bytes(index);
            if bytes == 0 {
                continue;
            }
            let stale = runtime.session.region_stale_base(index).unwrap_or(false);
            dirty_list.push(format!(
                "0x{:x}-0x{:x} {bytes}B{}",
                region.start,
                region.end,
                if stale { " [stale-base]" } else { "" }
            ));
        }
        if !dirty_list.is_empty() {
            lines.push(format!("dirty regions: {}", dirty_list.join(", ")));
        }

        let freeze = if runtime.session.is_frozen() {
            format!("frozen [depth {}]", runtime.session.freeze_depth())
        } else {
            "running".to_owned()
        };
        lines.push(format!("target {freeze}"));

        lines.join("\n")
    }

    #[cfg(not(feature = "memory"))]
    pub(crate) fn open_selected_memory_region(&mut self) -> crate::error::HxResult<()> {
        self.open_memory_panel("memory feature is not enabled");
        Ok(())
    }
}
