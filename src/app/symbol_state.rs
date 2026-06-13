#![cfg_attr(not(feature = "symbols"), allow(dead_code))]

use crate::app::SidePanelKind;
use crate::app::{App, SymbolState};
use crate::error::HxResult;
use crate::executable::{SymbolSource, SymbolType};
use crate::mode::Mode;

#[derive(Debug, Clone)]
pub(crate) struct SymbolPanelEntry {
    pub address: u64,
    pub name: String,
    pub name_kind: SymbolNameKind,
    pub size: u64,
    pub symbol_type: SymbolType,
    pub source: SymbolPanelEntrySource,
    pub logical_offset: Option<u64>,
    pub file_offset: Option<u64>,
    pub confidence_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolNameKind {
    Real,
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolPanelSource {
    Native,
    Sagitta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "sagitta-analysis"), allow(dead_code))]
pub(crate) enum SymbolPanelEntrySource {
    Object,
    Dynamic,
    Export,
    Sagitta,
}

impl From<SymbolSource> for SymbolPanelEntrySource {
    fn from(source: SymbolSource) -> Self {
        match source {
            SymbolSource::Object => Self::Object,
            SymbolSource::Dynamic => Self::Dynamic,
            SymbolSource::Export => Self::Export,
        }
    }
}

impl SymbolPanelEntry {
    pub(crate) fn native_entries(info: &crate::executable::ExecutableInfo) -> Vec<Self> {
        let mut entries =
            Vec::with_capacity(info.symbols_by_va.len() + info.target_names_by_va.len());
        for (&address, symbol) in &info.symbols_by_va {
            entries.push(SymbolPanelEntry {
                address,
                name: symbol.display_name.clone(),
                name_kind: SymbolNameKind::Real,
                size: symbol.size,
                symbol_type: symbol.symbol_type,
                source: symbol.source.into(),
                logical_offset: None,
                file_offset: info.file_offset_for_virtual(address),
                confidence_label: None,
            });
        }
        for (&address, name) in info.target_names_by_va.iter() {
            entries.push(SymbolPanelEntry {
                address,
                name: name.clone(),
                name_kind: SymbolNameKind::Real,
                size: 0,
                symbol_type: SymbolType::Function,
                source: SymbolPanelEntrySource::Dynamic,
                logical_offset: None,
                file_offset: info.file_offset_for_virtual(address),
                confidence_label: None,
            });
        }
        entries.sort_by_key(|entry| entry.address);
        entries
    }
}

impl SymbolState {
    pub(crate) fn native(info: crate::executable::ExecutableInfo) -> Self {
        Self::from_entries(
            SymbolPanelEntry::native_entries(&info),
            SymbolPanelSource::Native,
        )
    }

    pub(crate) fn from_entries(entries: Vec<SymbolPanelEntry>, source: SymbolPanelSource) -> Self {
        Self {
            entries,
            source,
            scroll_offset: 0,
            selected_row: 0,
            detail_scroll_offset: 0,
        }
    }

    pub(crate) fn row_count(&self) -> usize {
        self.entries.len()
    }
}

impl App {
    pub(crate) fn symbol_state(&self) -> Option<&SymbolState> {
        self.symbol_state.as_ref()
    }

    pub(crate) fn symbol_state_mut(&mut self) -> Option<&mut SymbolState> {
        self.symbol_state.as_mut()
    }

    pub(crate) fn focus_symbol_panel(&mut self) {
        self.clear_diff_cell_selection();
        self.active_side_panel = SidePanelKind::Symbol;
        self.mode = Mode::SidePanel;
        self.ensure_symbol_selection_visible();
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
        let (selected_row, scroll_offset) = match self.symbol_state() {
            Some(state) => (state.selected_row, state.scroll_offset),
            None => return,
        };
        let visible_rows = self.symbol_list_visible_rows();

        if let Some(state) = self.symbol_state_mut() {
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
        let Some(entry) = self
            .symbol_state()
            .and_then(|state| state.entries.get(state.selected_row))
            .cloned()
        else {
            return Ok(());
        };
        #[cfg(feature = "sagitta-analysis")]
        if self.sagitta_symbol_offsets_invalid() {
            return Err(crate::error::HxError::CommandError(
                "analysis offsets changed; rerun :ana".to_owned(),
            ));
        }

        let Some(offset) = entry.logical_offset.or(entry.file_offset) else {
            return Err(crate::error::HxError::CommandError(
                "symbol address not in mapped section".to_owned(),
            ));
        };
        let offset = if entry.logical_offset.is_some() {
            self.document
                .display_offset_for_logical_offset(offset)
                .ok_or_else(|| {
                    crate::error::HxError::CommandError(
                        "analysis target is unavailable; rerun :ana".to_owned(),
                    )
                })?
        } else {
            offset
        };

        let target_offset = self.clamp_offset(offset);
        self.cursor = target_offset;
        self.center_cursor_in_view();
        self.sync_inspector_to_cursor();

        #[cfg(feature = "sagitta-analysis")]
        if self.sagitta_symbol_bytes_outdated() {
            self.set_warning_status(format!(
                "jumped to {} @ 0x{:x}; analysis outdated; rerun :ana",
                entry.name, offset
            ));
            return Ok(());
        }
        self.set_info_status(format!("jumped to {} @ 0x{:x}", entry.name, offset));
        Ok(())
    }
}
