use crate::app::App;
use crate::error::HxResult;
use crate::mode::Mode;

impl App {
    pub(crate) fn move_horizontal(&mut self, delta: i64) -> HxResult<()> {
        self.ensure_insert_pending_committed()?;
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = crate::mode::NibblePhase::High;
        }
        Ok(())
    }

    pub(crate) fn move_vertical(&mut self, rows: i64) -> HxResult<()> {
        self.ensure_insert_pending_committed()?;
        let delta = rows.saturating_mul(self.config.bytes_per_line as i64);
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = crate::mode::NibblePhase::High;
        }
        Ok(())
    }

    pub(crate) fn move_row_edge(&mut self, end: bool) -> HxResult<()> {
        self.ensure_insert_pending_committed()?;
        let row_start = align_offset(self.cursor, self.config.bytes_per_line);
        let target = if end {
            row_start + self.config.bytes_per_line.saturating_sub(1) as u64
        } else {
            row_start
        };
        self.cursor = self.clamp_cursor_for_mode(target, self.mode);
        Ok(())
    }

    pub(crate) fn ensure_cursor_visible(&mut self) {
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

    pub(crate) fn scroll_viewport(&mut self, rows: i64) {
        if self.document.len() == 0 {
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
        if self.document.len() == 0 {
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
        if self.document.len() == 0 {
            return 0;
        }
        let row_size = self.config.bytes_per_line as u64;
        let visible_rows = self.visible_rows();
        let tail_rows = self.document.len().saturating_sub(1) / row_size;
        tail_rows
            .saturating_sub(visible_rows.saturating_sub(1))
            .saturating_mul(row_size)
    }

    pub(crate) fn visible_rows(&self) -> u64 {
        self.view_rows.max(1) as u64
    }

    pub(crate) fn offset_with_delta(&self, current: u64, delta: i64) -> u64 {
        if self.document.len() == 0 {
            return 0;
        }
        let max = self.cursor_max(self.mode_allows_eof_cursor());
        if delta >= 0 {
            current.saturating_add(delta as u64).min(max)
        } else {
            current.saturating_sub(delta.unsigned_abs()).min(max)
        }
    }

    pub(crate) fn clamp_offset(&self, offset: u64) -> u64 {
        self.clamp_offset_with_eof(offset, false)
    }

    pub(crate) fn clamp_cursor_for_mode(&self, offset: u64, mode: Mode) -> u64 {
        self.clamp_offset_with_eof(
            offset,
            matches!(mode, Mode::EditHex { .. } | Mode::InsertHex { .. }),
        )
    }

    pub(crate) fn mode_allows_eof_cursor(&self) -> bool {
        matches!(self.mode, Mode::EditHex { .. } | Mode::InsertHex { .. })
    }

    pub(crate) fn cursor_anchor_offset(&self) -> u64 {
        if self.document.len() == 0 {
            0
        } else {
            self.cursor.min(self.document.len() - 1)
        }
    }

    fn cursor_max(&self, allow_eof: bool) -> u64 {
        if self.document.len() == 0 {
            0
        } else if allow_eof {
            self.document.len()
        } else {
            self.document.len() - 1
        }
    }

    fn clamp_offset_with_eof(&self, offset: u64, allow_eof: bool) -> u64 {
        if self.document.len() == 0 {
            0
        } else {
            offset.min(self.cursor_max(allow_eof))
        }
    }
}

pub(crate) fn align_offset(offset: u64, bytes_per_line: usize) -> u64 {
    if bytes_per_line == 0 {
        offset
    } else {
        offset / bytes_per_line as u64 * bytes_per_line as u64
    }
}
