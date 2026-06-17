use std::path::PathBuf;

use super::range::ExecRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecArtifact {
    Bytes(Vec<u8>),
    Text(String),
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecOutcome {
    pub summary: String,
    pub cursor: Option<u64>,
    pub selection: Option<ExecRange>,
    pub bytes_read: Option<u64>,
    pub bytes_changed: Option<u64>,
    pub dirty: bool,
    pub warnings: Vec<String>,
    pub artifacts: Vec<ExecArtifact>,
}

impl ExecOutcome {
    pub fn new(summary: impl Into<String>, dirty: bool) -> Self {
        Self {
            summary: summary.into(),
            dirty,
            ..Self::default()
        }
    }
}
