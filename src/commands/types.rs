use std::path::PathBuf;

use crate::copy::{CopyDisplay, CopyFormat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportFormat {
    Binary { path: PathBuf },
    CArray { name: String },
    PythonBytes { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GotoTarget {
    Absolute(u64),
    Relative(i64),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Crc32,
}

impl HashAlgorithm {
    pub fn label(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
            Self::Crc32 => "crc32",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "md5" => Some(Self::Md5),
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            "sha512" => Some(Self::Sha512),
            "crc32" => Some(Self::Crc32),
            _ => None,
        }
    }
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
    Fill {
        pattern: Vec<u8>,
        len: usize,
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
    Export {
        format: ExportFormat,
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
    Hash {
        algorithm: HashAlgorithm,
    },
}
