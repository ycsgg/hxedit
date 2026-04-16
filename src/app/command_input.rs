use crate::app::text_cursor::{
    backspace_char_before_cursor, delete_char_at_cursor, insert_char_at_cursor, move_cursor_end,
    move_cursor_home, move_cursor_left, move_cursor_right,
};
use crate::app::App;
use crate::mode::Mode;

impl App {
    pub(crate) fn insert_command_char(&mut self, c: char) {
        insert_char_at_cursor(&mut self.command_buffer, &mut self.command_cursor_pos, c);
    }

    pub(crate) fn move_command_cursor_left(&mut self) {
        move_cursor_left(&self.command_buffer, &mut self.command_cursor_pos);
    }

    pub(crate) fn move_command_cursor_right(&mut self) {
        move_cursor_right(&self.command_buffer, &mut self.command_cursor_pos);
    }

    pub(crate) fn move_command_cursor_home(&mut self) {
        move_cursor_home(&mut self.command_cursor_pos);
    }

    pub(crate) fn move_command_cursor_end(&mut self) {
        move_cursor_end(&self.command_buffer, &mut self.command_cursor_pos);
    }

    pub(crate) fn delete_command_char(&mut self) {
        delete_char_at_cursor(&mut self.command_buffer, self.command_cursor_pos);
    }

    pub(crate) fn backspace_command_char(&mut self) {
        backspace_char_before_cursor(&mut self.command_buffer, &mut self.command_cursor_pos);
    }

    pub(crate) fn cancel_command_input(&mut self) {
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        let return_mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
        self.mode = self.normalize_mode(return_mode);
    }
}
