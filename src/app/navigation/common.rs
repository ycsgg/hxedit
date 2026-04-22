use crate::app::App;
use crate::mode::Mode;

impl App {
    pub(crate) fn visible_rows(&self) -> u64 {
        self.view_rows.max(1) as u64
    }

    pub(crate) fn offset_with_delta(&self, current: u64, delta: i64) -> u64 {
        if self.document.is_empty() {
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
        if self.document.is_empty() {
            0
        } else {
            self.cursor.min(self.document.len() - 1)
        }
    }

    fn cursor_max(&self, allow_eof: bool) -> u64 {
        if self.document.is_empty() {
            0
        } else if allow_eof {
            self.document.len()
        } else {
            self.document.len() - 1
        }
    }

    fn clamp_offset_with_eof(&self, offset: u64, allow_eof: bool) -> u64 {
        if self.document.is_empty() {
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
