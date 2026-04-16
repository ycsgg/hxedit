use crate::app::App;
use crate::error::HxResult;
use crate::mode::Mode;

impl App {
    pub(crate) fn insert_command_char(&mut self, c: char) -> HxResult<()> {
        let pos = self.command_cursor_pos.min(self.command_buffer.len());
        self.command_buffer.insert(pos, c);
        self.command_cursor_pos = pos + c.len_utf8();
        Ok(())
    }

    pub(crate) fn move_command_cursor_left(&mut self) -> HxResult<()> {
        self.command_cursor_pos = prev_char_boundary(&self.command_buffer, self.command_cursor_pos);
        Ok(())
    }

    pub(crate) fn move_command_cursor_right(&mut self) -> HxResult<()> {
        self.command_cursor_pos = next_char_boundary(&self.command_buffer, self.command_cursor_pos);
        Ok(())
    }

    pub(crate) fn move_command_cursor_home(&mut self) -> HxResult<()> {
        self.command_cursor_pos = 0;
        Ok(())
    }

    pub(crate) fn move_command_cursor_end(&mut self) -> HxResult<()> {
        self.command_cursor_pos = self.command_buffer.len();
        Ok(())
    }

    pub(crate) fn delete_command_char(&mut self) -> HxResult<()> {
        if self.command_cursor_pos < self.command_buffer.len() {
            let next = next_char_boundary(&self.command_buffer, self.command_cursor_pos);
            self.command_buffer
                .replace_range(self.command_cursor_pos..next, "");
        }
        Ok(())
    }

    pub(crate) fn backspace_command_char(&mut self) -> HxResult<()> {
        if self.command_cursor_pos > 0 {
            let prev = prev_char_boundary(&self.command_buffer, self.command_cursor_pos);
            self.command_buffer
                .replace_range(prev..self.command_cursor_pos, "");
            self.command_cursor_pos = prev;
        }
        Ok(())
    }

    pub(crate) fn cancel_command_input(&mut self) -> HxResult<()> {
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        let return_mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
        self.mode = self.normalize_mode(return_mode);
        Ok(())
    }
}

fn prev_char_boundary(text: &str, cursor_pos: usize) -> usize {
    text[..cursor_pos.min(text.len())]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor_pos: usize) -> usize {
    if cursor_pos >= text.len() {
        return text.len();
    }
    let start = cursor_pos.min(text.len());
    text[start..]
        .char_indices()
        .nth(1)
        .map(|(idx, _)| start + idx)
        .unwrap_or(text.len())
}
