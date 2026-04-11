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
    #[error("invalid hex pattern: {0}")]
    InvalidHexPattern(String),
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("missing command argument: {0}")]
    MissingArgument(&'static str),
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
