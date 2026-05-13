use crate::app::SidePanelKind;
use crate::app::{App, DataState};
use crate::core::document::ByteSlot;
use crate::mode::Mode;

const DATA_READ_BYTES: usize = 16;

impl App {
    pub(crate) fn data_state(&self) -> Option<&DataState> {
        self.data_state.as_ref()
    }

    pub(crate) fn data_state_mut(&mut self) -> Option<&mut DataState> {
        self.data_state.as_mut()
    }

    pub(crate) fn refresh_data_panel(&mut self) {
        if self.data_state.is_none() {
            return;
        }
        let previous = self.data_state().cloned();
        self.data_state = Some(DataState {
            base_offset: self.cursor_anchor_offset(),
            bytes: self.read_data_panel_bytes(),
            scroll_offset: previous
                .as_ref()
                .map(|state| state.scroll_offset)
                .unwrap_or(0),
            selected_label: previous.and_then(|state| state.selected_label),
        });
    }

    pub(crate) fn open_data_panel(&mut self) {
        self.clear_diff_cell_selection();
        self.show_side_panel = true;
        self.data_state = Some(DataState {
            base_offset: self.cursor_anchor_offset(),
            bytes: self.read_data_panel_bytes(),
            scroll_offset: 0,
            selected_label: None,
        });
        self.focus_data_panel();
        self.set_info_status("data panel opened at cursor");
    }

    pub(crate) fn close_data_panel(&mut self) {
        if self.data_state.is_some() {
            self.data_state = None;
            self.restore_inspector_after_side_panel_close();
            self.show_side_panel = false;
            if self.mode.is_side_panel() {
                self.mode = Mode::Normal;
            }
            self.clear_status();
        }
    }

    pub(crate) fn scroll_data_panel(&mut self, rows: i64) {
        let visible_rows = self.side_panel_visible_rows();
        let total_rows = crate::view::data_panel::line_count();
        let Some(state) = self.data_state_mut() else {
            return;
        };
        let max_scroll = total_rows.saturating_sub(visible_rows);
        state.scroll_offset = if rows >= 0 {
            state
                .scroll_offset
                .saturating_add(rows as usize)
                .min(max_scroll)
        } else {
            state.scroll_offset.saturating_sub((-rows) as usize)
        };
    }

    pub(crate) fn focus_data_panel(&mut self) {
        self.active_side_panel = SidePanelKind::Data;
        self.mode = Mode::SidePanel;
        self.refresh_data_panel();
    }

    pub(crate) fn move_data_panel_selection(&mut self, delta: i64) {
        let current = self
            .data_state()
            .and_then(|state| {
                state
                    .selected_label
                    .as_deref()
                    .and_then(position_for_label)
                    .map(|index| index + 1)
            })
            .unwrap_or(1);
        let last = crate::view::data_panel::line_count()
            .saturating_sub(1)
            .max(1);
        let target = if delta >= 0 {
            current.saturating_add(delta as usize).min(last)
        } else {
            current.saturating_sub((-delta) as usize).max(1)
        };
        self.select_data_panel_row(target);
    }

    pub(crate) fn select_data_panel_row(&mut self, visual_row: usize) {
        let Some(label) = crate::view::data_panel::label_at_visual_row(visual_row) else {
            return;
        };
        let len = self
            .data_state()
            .and_then(|state| crate::view::data_panel::byte_len_for_label(label, state));
        let Some(len) = len else {
            if let Some(state) = self.data_state_mut() {
                state.selected_label = Some(label.to_owned());
            }
            self.set_warning_status(format!("data {label}: unavailable at cursor"));
            return;
        };
        let base_offset = self.cursor_anchor_offset();
        if let Some(state) = self.data_state_mut() {
            state.selected_label = Some(label.to_owned());
        }
        self.selection_anchor = Some(base_offset);
        self.cursor = self.clamp_offset(base_offset + len.saturating_sub(1) as u64);
        self.mode = Mode::Visual;
        self.ensure_cursor_visible();
        self.set_info_status(format!("selected data {label} [{} bytes]", len));
    }

    fn read_data_panel_bytes(&mut self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(DATA_READ_BYTES);
        let start = self.cursor_anchor_offset();
        for offset in start..start.saturating_add(DATA_READ_BYTES as u64) {
            match self.document.byte_at(offset) {
                Ok(ByteSlot::Present(byte)) => bytes.push(byte),
                Ok(ByteSlot::Deleted | ByteSlot::Empty) | Err(_) => break,
            }
        }
        bytes
    }
}

fn position_for_label(label: &str) -> Option<usize> {
    (1..crate::view::data_panel::line_count())
        .find(|visual_row| crate::view::data_panel::label_at_visual_row(*visual_row) == Some(label))
}
