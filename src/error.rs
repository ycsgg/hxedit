use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HxError {
    #[error("document is read-only")]
    ReadOnly,
    #[error("buffer has unsaved changes; use :q! to force quit")]
    DirtyQuit,
    #[error("offset is outside the current document")]
    OffsetOutOfRange,
    #[error("search pattern must not be empty")]
    EmptySearch,
    #[error("invalid offset: {0}")]
    InvalidOffset(String),
    #[error("invalid fill count: {0}")]
    InvalidFillCount(String),
    #[error("invalid replace command: {0}")]
    InvalidReplace(String),
    #[error("invalid paste count: {0}")]
    InvalidPasteCount(String),
    #[error("invalid paste data: {0}")]
    InvalidPasteData(String),
    #[error("invalid copy format: {0}")]
    InvalidCopyFormat(String),
    #[error("invalid copy display: {0}")]
    InvalidCopyDisplay(String),
    #[error("invalid undo count: {0}")]
    InvalidUndoCount(String),
    #[error("invalid redo count: {0}")]
    InvalidRedoCount(String),
    #[error("invalid hash algorithm: {0}; supported: md5 sha1 sha256 sha512 crc32")]
    InvalidHashAlgorithm(String),
    #[error("invalid hex pattern: {0}")]
    InvalidHexPattern(String),
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("missing command argument: {0}")]
    MissingArgument(&'static str),
    #[error("operation requires an active visual selection")]
    MissingSelection,
    #[error("selection length must be a multiple of {0} bytes for this copy mode")]
    CopyAlignment(usize),
    #[error("clipboard error: {0}")]
    Clipboard(String),
    #[error("failed to open {path}: {source}")]
    OpenPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type HxResult<T> = Result<T, HxError>;
