use crate::app::{App, MainView};
use crate::mode::Mode;

use super::align_offset;

impl App {
    pub(crate) fn move_horizontal(&mut self, delta: i64) {
        self.ensure_insert_pending_committed();
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = crate::mode::NibblePhase::High;
        }
    }

    pub(crate) fn move_vertical(&mut self, rows: i64) {
        self.ensure_insert_pending_committed();
        if matches!(self.main_view, MainView::Disassembly(_)) {
            self.move_vertical_disassembly(rows);
            return;
        }
        let delta = rows.saturating_mul(self.config.bytes_per_line as i64);
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = crate::mode::NibblePhase::High;
        }
    }

    pub(crate) fn move_row_edge(&mut self, end: bool) {
        self.ensure_insert_pending_committed();
        let row_start = align_offset(self.cursor, self.config.bytes_per_line);
        let target = if end {
            row_start + self.config.bytes_per_line.saturating_sub(1) as u64
        } else {
            row_start
        };
        self.cursor = self.clamp_cursor_for_mode(target, self.mode);
    }

    pub(crate) fn ensure_cursor_visible(&mut self) {
        if matches!(self.main_view, MainView::Disassembly(_)) {
            if let Err(err) = self.ensure_cursor_visible_disassembly() {
                self.set_error_status(err.to_string());
            }
            return;
        }
        let row_size = self.config.bytes_per_line as u64;
        let cursor_row = align_offset(self.cursor_anchor_offset(), self.config.bytes_per_line);
        let visible_rows = self.visible_rows();
        let bottom = self.viewport_top + visible_rows.saturating_sub(1) * row_size;
        if cursor_row < self.viewport_top {
            self.viewport_top = cursor_row;
        } else if cursor_row > bottom {
            self.viewport_top =
                cursor_row.saturating_sub((visible_rows.saturating_sub(1)) * row_size);
        }
        self.viewport_top = align_offset(self.viewport_top, self.config.bytes_per_line);
    }

    pub(crate) fn center_cursor_in_view(&mut self) {
        if matches!(self.main_view, MainView::Disassembly(_)) {
            if let Err(err) = self.center_cursor_in_view_disassembly() {
                self.set_error_status(err.to_string());
            }
            return;
        }
        if self.document.is_empty() {
            self.viewport_top = 0;
            return;
        }
        let row_size = self.config.bytes_per_line as u64;
        let cursor_row = align_offset(self.cursor_anchor_offset(), self.config.bytes_per_line);
        let visible_rows = self.visible_rows();
        let bottom = self.viewport_top + visible_rows.saturating_sub(1) * row_size;
        if cursor_row >= self.viewport_top && cursor_row <= bottom {
            return;
        }
        let center_rows = visible_rows / 2;
        let max_top = self.max_viewport_top();
        self.viewport_top = align_offset(
            cursor_row.saturating_sub(center_rows.saturating_mul(row_size)),
            self.config.bytes_per_line,
        )
        .min(max_top);
    }

    pub(crate) fn scroll_viewport(&mut self, rows: i64) {
        if self.document.is_empty() {
            return;
        }
        if matches!(self.main_view, MainView::Disassembly(_)) {
            if let Err(err) = self.scroll_viewport_disassembly(rows) {
                self.set_error_status(err.to_string());
            }
            return;
        }
        let max_top = self.max_viewport_top();
        let delta = rows.saturating_mul(self.config.bytes_per_line as i64);
        self.viewport_top = if delta >= 0 {
            self.viewport_top.saturating_add(delta as u64).min(max_top)
        } else {
            self.viewport_top.saturating_sub(delta.unsigned_abs())
        };
        self.viewport_top =
            align_offset(self.viewport_top, self.config.bytes_per_line).min(max_top);
        self.clamp_cursor_into_view();
    }

    pub(crate) fn clamp_cursor_into_view(&mut self) {
        if matches!(self.main_view, MainView::Disassembly(_)) {
            if let Err(err) = self.clamp_cursor_into_view_disassembly() {
                self.set_error_status(err.to_string());
            }
            return;
        }
        if self.document.is_empty() {
            self.cursor = 0;
            return;
        }
        let row_size = self.config.bytes_per_line as u64;
        let visible_rows = self.visible_rows();
        let visible_start = self.viewport_top;
        let visible_end = (self.viewport_top + visible_rows.saturating_mul(row_size))
            .min(self.document.len())
            .saturating_sub(1);
        let anchor = self
            .cursor_anchor_offset()
            .clamp(visible_start, visible_end);
        if self.mode_allows_eof_cursor()
            && self.cursor == self.document.len()
            && self.cursor_anchor_offset() == anchor
        {
            return;
        }
        self.cursor = anchor;
    }

    pub(crate) fn max_viewport_top(&self) -> u64 {
        if self.document.is_empty() {
            return 0;
        }
        let row_size = self.config.bytes_per_line as u64;
        let visible_rows = self.visible_rows();
        let tail_rows = self.document.len().saturating_sub(1) / row_size;
        tail_rows
            .saturating_sub(visible_rows.saturating_sub(1))
            .saturating_mul(row_size)
    }
}
