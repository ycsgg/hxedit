use std::path::PathBuf;

use crate::copy::{CopyDisplay, CopyFormat};

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
        offset: u64,
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
    Copy {
        format: CopyFormat,
        display: CopyDisplay,
    },
    SearchAscii {
        pattern: Vec<u8>,
    },
    SearchHex {
        pattern: Vec<u8>,
    },
}
