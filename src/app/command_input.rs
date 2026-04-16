use crate::app::text_cursor::{next_char_boundary, prev_char_boundary};
use crate::app::App;
use crate::mode::Mode;

impl App {
    pub(crate) fn insert_command_char(&mut self, c: char) {
        let pos = self.command_cursor_pos.min(self.command_buffer.len());
        self.command_buffer.insert(pos, c);
        self.command_cursor_pos = pos + c.len_utf8();
    }

    pub(crate) fn move_command_cursor_left(&mut self) {
        self.command_cursor_pos = prev_char_boundary(&self.command_buffer, self.command_cursor_pos);
    }

    pub(crate) fn move_command_cursor_right(&mut self) {
        self.command_cursor_pos = next_char_boundary(&self.command_buffer, self.command_cursor_pos);
    }

    pub(crate) fn move_command_cursor_home(&mut self) {
        self.command_cursor_pos = 0;
    }

    pub(crate) fn move_command_cursor_end(&mut self) {
        self.command_cursor_pos = self.command_buffer.len();
    }

    pub(crate) fn delete_command_char(&mut self) {
        if self.command_cursor_pos < self.command_buffer.len() {
            let next = next_char_boundary(&self.command_buffer, self.command_cursor_pos);
            self.command_buffer
                .replace_range(self.command_cursor_pos..next, "");
        }
    }

    pub(crate) fn backspace_command_char(&mut self) {
        if self.command_cursor_pos > 0 {
            let prev = prev_char_boundary(&self.command_buffer, self.command_cursor_pos);
            self.command_buffer
                .replace_range(prev..self.command_cursor_pos, "");
            self.command_cursor_pos = prev;
        }
    }

    pub(crate) fn cancel_command_input(&mut self) {
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        let return_mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
        self.mode = self.normalize_mode(return_mode);
    }
}
