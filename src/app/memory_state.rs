use crate::app::{App, SidePanelKind, StatusLevel};
use crate::mode::Mode;

#[cfg(feature = "memory")]
pub(crate) struct MemoryRuntime {
    pub(crate) session: crate::memory::MemorySession,
    pub(crate) selected_region: usize,
    pub(crate) opened_region: usize,
    pub(crate) base_va: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryPanelState {
    pub(crate) message: String,
    pub(crate) scroll_offset: usize,
    pub(crate) selected_row: usize,
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
            message: message.clone(),
            scroll_offset: 0,
            selected_row,
        });
        self.active_side_panel = SidePanelKind::Memory;
        self.show_side_panel = true;
        self.mode = Mode::SidePanel;
        self.set_status(StatusLevel::Notice, message);
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
            message: message.clone(),
            scroll_offset: 0,
            selected_row,
        });
        self.active_side_panel = SidePanelKind::Memory;
        self.show_side_panel = true;
        self.mode = Mode::Normal;
        self.set_status(StatusLevel::Notice, message);
    }

    #[cfg(feature = "memory")]
    pub(crate) fn move_memory_selection(&mut self, delta: isize) {
        let Some(runtime) = self.memory_runtime.as_mut() else {
            return;
        };
        let count = runtime.session.regions().count();
        if count == 0 {
            runtime.selected_region = 0;
            return;
        }
        let current = runtime.selected_region.min(count - 1);
        let next = if delta < 0 {
            current.saturating_sub((-delta) as usize)
        } else {
            current.saturating_add(delta as usize).min(count - 1)
        };
        runtime.selected_region = next;
        if let Some(state) = self.memory_state.as_mut() {
            state.selected_row = next;
            let height = self.view_rows.max(1);
            if next < state.scroll_offset {
                state.scroll_offset = next;
            } else if next >= state.scroll_offset + height {
                state.scroll_offset = next.saturating_sub(height - 1);
            }
        }
    }

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
        let document = {
            let runtime = self.memory_runtime.as_mut().expect("checked above");
            let document = runtime.session.document_for_region(region_index, &config)?;
            runtime.selected_region = region_index;
            runtime.opened_region = region_index;
            runtime.base_va = region.start;
            document
        };
        self.document = document;
        self.cursor = self.clamp_cursor_for_mode(addr.saturating_sub(region.start), self.mode);
        self.viewport_top =
            super::navigation::align_offset(self.cursor, self.config.bytes_per_line);
        self.selection_anchor = None;
        self.mouse_selection_anchor = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.document_revision = self.document_revision.saturating_add(1);
        self.invalidate_disassembly_cache();
        self.refresh_inspector();
        self.sync_memory_panel_selection(region_index);
        Ok(())
    }

    #[cfg(feature = "memory")]
    fn sync_memory_panel_selection(&mut self, region_index: usize) {
        if let Some(state) = self.memory_state.as_mut() {
            state.selected_row = region_index;
            let height = self.view_rows.max(1);
            if region_index < state.scroll_offset {
                state.scroll_offset = region_index;
            } else if region_index >= state.scroll_offset + height {
                state.scroll_offset = region_index.saturating_sub(height - 1);
            }
        }
    }

    #[cfg(feature = "memory")]
    pub(crate) fn commit_memory_document(
        &mut self,
        commit_all: bool,
    ) -> crate::error::HxResult<()> {
        if !self.document.is_fixed_size() {
            self.open_memory_panel("memory commit requires an active memory document");
            return Ok(());
        }
        let spans = self.document.replacement_spans();
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

        let command_label = if commit_all { "commit-all" } else { "commit" };
        let mut message = format!(
            "memory {command_label} wrote {total_bytes} byte{} across {} span{} at 0x{:x}-0x{:x}",
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

    #[cfg(not(feature = "memory"))]
    pub(crate) fn open_selected_memory_region(&mut self) -> crate::error::HxResult<()> {
        self.open_memory_panel("memory feature is not enabled");
        Ok(())
    }
}
