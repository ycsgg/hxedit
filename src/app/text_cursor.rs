pub(crate) fn prev_char_boundary(text: &str, cursor_pos: usize) -> usize {
    text[..cursor_pos.min(text.len())]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

pub(crate) fn next_char_boundary(text: &str, cursor_pos: usize) -> usize {
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

pub(crate) fn insert_char_at_cursor(text: &mut String, cursor_pos: &mut usize, c: char) {
    let pos = (*cursor_pos).min(text.len());
    text.insert(pos, c);
    *cursor_pos = pos + c.len_utf8();
}

pub(crate) fn move_cursor_left(text: &str, cursor_pos: &mut usize) {
    *cursor_pos = prev_char_boundary(text, *cursor_pos);
}

pub(crate) fn move_cursor_right(text: &str, cursor_pos: &mut usize) {
    *cursor_pos = next_char_boundary(text, *cursor_pos);
}

pub(crate) fn move_cursor_home(cursor_pos: &mut usize) {
    *cursor_pos = 0;
}

pub(crate) fn move_cursor_end(text: &str, cursor_pos: &mut usize) {
    *cursor_pos = text.len();
}

pub(crate) fn delete_char_at_cursor(text: &mut String, cursor_pos: usize) {
    let pos = cursor_pos.min(text.len());
    if pos < text.len() {
        let next = next_char_boundary(text, pos);
        text.replace_range(pos..next, "");
    }
}

pub(crate) fn backspace_char_before_cursor(text: &mut String, cursor_pos: &mut usize) {
    let pos = (*cursor_pos).min(text.len());
    if pos > 0 {
        let prev = prev_char_boundary(text, pos);
        text.replace_range(prev..pos, "");
        *cursor_pos = prev;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        backspace_char_before_cursor, delete_char_at_cursor, insert_char_at_cursor,
        move_cursor_left, move_cursor_right,
    };

    #[test]
    fn shared_text_edit_ops_preserve_utf8_boundaries() {
        let mut text = "a中b".to_owned();
        let mut cursor_pos = text.len();

        move_cursor_left(&text, &mut cursor_pos);
        assert_eq!(cursor_pos, 4);

        backspace_char_before_cursor(&mut text, &mut cursor_pos);
        assert_eq!(text, "ab");
        assert_eq!(cursor_pos, 1);

        insert_char_at_cursor(&mut text, &mut cursor_pos, '界');
        assert_eq!(text, "a界b");
        assert_eq!(cursor_pos, 4);

        delete_char_at_cursor(&mut text, cursor_pos);
        assert_eq!(text, "a界");

        cursor_pos = 0;
        move_cursor_right(&text, &mut cursor_pos);
        assert_eq!(cursor_pos, 1);
    }
}
