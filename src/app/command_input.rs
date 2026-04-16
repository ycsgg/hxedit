use crate::app::text_cursor::{
    backspace_char_before_cursor, delete_char_at_cursor, insert_char_at_cursor, move_cursor_end,
    move_cursor_home, move_cursor_left, move_cursor_right,
};
use crate::app::App;
use crate::mode::Mode;

impl App {
    pub(crate) fn reset_command_history_navigation(&mut self) {
        self.command_history_index = None;
        self.command_history_stash = None;
    }

    fn command_buffer_did_change(&mut self) {
        self.reset_command_history_navigation();
    }

    pub(crate) fn remember_command_submission(&mut self) {
        let command = self.command_buffer.trim().to_owned();
        if command.is_empty() {
            return;
        }

        if self.command_history.last() != Some(&command) {
            self.command_history.push(command);
        }
        self.reset_command_history_navigation();
    }

    pub(crate) fn insert_command_char(&mut self, c: char) {
        self.command_buffer_did_change();
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
        self.command_buffer_did_change();
        delete_char_at_cursor(&mut self.command_buffer, self.command_cursor_pos);
    }

    pub(crate) fn backspace_command_char(&mut self) {
        self.command_buffer_did_change();
        backspace_char_before_cursor(&mut self.command_buffer, &mut self.command_cursor_pos);
    }

    pub(crate) fn command_history_prev(&mut self) {
        let Some(last_index) = self.command_history.len().checked_sub(1) else {
            return;
        };

        let next_index = match self.command_history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.command_history_stash = Some(self.command_buffer.clone());
                last_index
            }
        };
        self.command_history_index = Some(next_index);
        self.command_buffer = self.command_history[next_index].clone();
        self.command_cursor_pos = self.command_buffer.len();
    }

    pub(crate) fn command_history_next(&mut self) {
        let Some(index) = self.command_history_index else {
            return;
        };

        if index + 1 < self.command_history.len() {
            let next_index = index + 1;
            self.command_history_index = Some(next_index);
            self.command_buffer = self.command_history[next_index].clone();
        } else {
            self.command_history_index = None;
            self.command_buffer = self.command_history_stash.take().unwrap_or_default();
        }
        self.command_cursor_pos = self.command_buffer.len();
    }

    pub(crate) fn cancel_command_input(&mut self) {
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        self.reset_command_history_navigation();
        let return_mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
        self.mode = self.normalize_mode(return_mode);
    }
}
