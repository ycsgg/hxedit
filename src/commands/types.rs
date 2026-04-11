use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit { force: bool },
    Write { path: Option<PathBuf> },
    WriteQuit { path: Option<PathBuf> },
    Goto { offset: u64 },
    Undo { steps: usize },
    SearchAscii { pattern: Vec<u8> },
    SearchHex { pattern: Vec<u8> },
}
