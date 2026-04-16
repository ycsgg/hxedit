use std::path::PathBuf;

use crate::copy::{CopyDisplay, CopyFormat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GotoTarget {
    Absolute(u64),
    Relative(i64),
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit {
        force: bool,
    },
    Write {
        path: Option<PathBuf>,
    },
    WriteQuit {
        path: Option<PathBuf>,
    },
    Goto {
        target: GotoTarget,
    },
    /// Overwrite-paste: writes clipboard bytes over existing content starting
    /// at cursor.  Does not shift subsequent offsets.
    Paste {
        raw: bool,
        preview: bool,
        limit: Option<usize>,
    },
    /// Insert-paste: inserts clipboard bytes at cursor, shifting subsequent
    /// offsets right.  Same arguments as `Paste`.
    PasteInsert {
        raw: bool,
        preview: bool,
        limit: Option<usize>,
    },
    Undo {
        steps: usize,
    },
    Redo {
        steps: usize,
    },
    Copy {
        format: CopyFormat,
        display: CopyDisplay,
    },
    SearchAscii {
        pattern: Vec<u8>,
        backward: bool,
    },
    SearchHex {
        pattern: Vec<u8>,
        backward: bool,
    },
    Inspector,
    Format {
        name: Option<String>,
    },
}
