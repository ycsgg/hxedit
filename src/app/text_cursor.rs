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
