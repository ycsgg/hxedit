use crate::core::document::ByteSlot;

pub fn offset_width(file_len: u64) -> usize {
    if file_len > u32::MAX as u64 {
        16
    } else {
        8
    }
}

pub fn format_offset(offset: u64, width: usize) -> String {
    format!("{offset:0width$x}", width = width)
}

pub fn ascii_char(slot: ByteSlot) -> char {
    match slot {
        ByteSlot::Present(byte) => {
            if byte.is_ascii_graphic() || byte == b' ' {
                byte as char
            } else if byte.is_ascii_whitespace() {
                '.'
            } else if byte.is_ascii() {
                '.'
            } else {
                '·'
            }
        }
        ByteSlot::Deleted => 'x',
        ByteSlot::Empty => ' ',
    }
}

pub fn hex_pair(slot: ByteSlot) -> [char; 2] {
    match slot {
        ByteSlot::Present(byte) => {
            let s = format!("{byte:02x}");
            let mut chars = s.chars();
            [chars.next().unwrap(), chars.next().unwrap()]
        }
        ByteSlot::Deleted => ['X', 'X'],
        ByteSlot::Empty => [' ', ' '],
    }
}
