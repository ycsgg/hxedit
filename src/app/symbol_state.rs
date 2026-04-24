use crate::app::{App, SidePanel, SymbolState};
use crate::error::HxResult;
use crate::executable::{SymbolSource, SymbolType};

#[derive(Debug, Clone)]
pub(crate) struct SymbolPanelEntry {
    pub address: u64,
    pub name: String,
    pub size: u64,
    pub symbol_type: SymbolType,
    pub source: SymbolSource,
    pub file_offset: Option<u64>,
}

impl SymbolState {
    pub(crate) fn entries(&self) -> Vec<SymbolPanelEntry> {
        let mut entries = Vec::with_capacity(self.row_count());
        for (&address, symbol) in &self.info.symbols_by_va {
            entries.push(SymbolPanelEntry {
                address,
                name: symbol.display_name.clone(),
                size: symbol.size,
                symbol_type: symbol.symbol_type,
                source: symbol.source,
                file_offset: self.info.file_offset_for_virtual(address),
            });
        }
        for (&address, name) in self.info.target_names_by_va.iter() {
            entries.push(SymbolPanelEntry {
                address,
                name: name.clone(),
                size: 0,
                symbol_type: SymbolType::Function,
                source: SymbolSource::Dynamic,
                file_offset: self.info.file_offset_for_virtual(address),
            });
        }
        entries.sort_by_key(|entry| entry.address);
        entries
    }

    pub(crate) fn row_count(&self) -> usize {
        self.info.symbols_by_va.len() + self.info.target_names_by_va.len()
    }
}

impl App {
    pub(crate) fn symbol_state(&self) -> Option<&SymbolState> {
        match &self.side_panel {
            Some(SidePanel::Symbol(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn symbol_state_mut(&mut self) -> Option<&mut SymbolState> {
        match &mut self.side_panel {
            Some(SidePanel::Symbol(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn move_symbol_selection(&mut self, delta: i64) {
        let Some(state) = self.symbol_state_mut() else {
            return;
        };
        let count = state.row_count();
        if count == 0 {
            return;
        }
        let new_row = if delta > 0 {
            state
                .selected_row
                .saturating_add(delta as usize)
                .min(count - 1)
        } else {
            state.selected_row.saturating_sub((-delta) as usize)
        };
        state.selected_row = new_row;
        state.detail_scroll_offset = 0;
        self.ensure_symbol_selection_visible();
    }

    pub(crate) fn ensure_symbol_selection_visible(&mut self) {
        let (selected_row, scroll_offset) = match &self.side_panel {
            Some(SidePanel::Symbol(state)) => (state.selected_row, state.scroll_offset),
            _ => return,
        };
        let visible_rows = self.symbol_list_visible_rows();

        if let Some(SidePanel::Symbol(state)) = &mut self.side_panel {
            if selected_row < scroll_offset {
                state.scroll_offset = selected_row;
            } else if selected_row >= scroll_offset + visible_rows {
                state.scroll_offset = selected_row.saturating_sub(visible_rows - 1);
            }
        }
    }

    pub(crate) fn scroll_symbol_panel(&mut self, rows: i64) {
        let visible_rows = self.symbol_list_visible_rows();
        let Some(state) = self.symbol_state_mut() else {
            return;
        };
        let max_scroll = state.row_count().saturating_sub(visible_rows);
        state.scroll_offset = if rows >= 0 {
            state
                .scroll_offset
                .saturating_add(rows as usize)
                .min(max_scroll)
        } else {
            state.scroll_offset.saturating_sub((-rows) as usize)
        };
    }

    pub(crate) fn set_symbol_selected_row(&mut self, row: usize) {
        let Some(state) = self.symbol_state_mut() else {
            return;
        };
        let max_row = state.row_count().saturating_sub(1);
        state.selected_row = row.min(max_row);
        state.detail_scroll_offset = 0;
        self.ensure_symbol_selection_visible();
    }

    pub(crate) fn scroll_symbol_detail(&mut self, rows: i64, width: u16) {
        let visible_rows = self.symbol_detail_visible_rows();
        let Some(state) = self.symbol_state_mut() else {
            return;
        };
        let detail_len = crate::view::symbol_panel::detail_line_count(state, width);
        let max_scroll = detail_len.saturating_sub(visible_rows);
        state.detail_scroll_offset = if rows >= 0 {
            state
                .detail_scroll_offset
                .saturating_add(rows as usize)
                .min(max_scroll)
        } else {
            state.detail_scroll_offset.saturating_sub((-rows) as usize)
        };
    }

    pub(crate) fn symbol_list_visible_rows(&self) -> usize {
        crate::view::symbol_panel::list_height(self.view_rows as u16)
            .saturating_sub(1)
            .max(1)
    }

    pub(crate) fn symbol_detail_visible_rows(&self) -> usize {
        crate::view::symbol_panel::detail_height(self.view_rows as u16).max(1)
    }

    /// Enter key navigates to the selected symbol's location.
    pub(crate) fn navigate_to_selected_symbol(&mut self) -> HxResult<()> {
        // Extract necessary data first to avoid borrowing issues
        let info = match &self.side_panel {
            Some(SidePanel::Symbol(state)) => Some(state.info.clone()),
            _ => None,
        };
        let selected_row = self.symbol_state().map(|s| s.selected_row);

        let (Some(info), Some(selected_row)) = (info, selected_row) else {
            return Ok(());
        };

        let state = SymbolState {
            info: info.clone(),
            scroll_offset: 0,
            selected_row,
            detail_scroll_offset: 0,
        };
        let entries = state.entries();
        let total = entries.len();

        if selected_row >= total {
            return Ok(());
        }

        let entry = &entries[selected_row];
        let address = entry.address;
        let name = entry.name.clone();

        // Convert virtual address to file offset
        let Some(offset) = info.file_offset_for_virtual(address) else {
            return Err(crate::error::HxError::CommandError(
                "symbol address not in mapped section".to_owned(),
            ));
        };

        // Navigate to the location
        let target_offset = self.clamp_offset(offset);
        self.cursor = target_offset;
        self.center_cursor_in_view();

        // Sync inspector (if switching back to inspector)
        self.sync_inspector_to_cursor();

        self.set_info_status(format!("jumped to {} @ 0x{:x}", name, offset));
        Ok(())
    }
}
