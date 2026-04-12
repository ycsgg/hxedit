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
