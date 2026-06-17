use std::path::{Path, PathBuf};

use crate::commands::types::HashAlgorithm;
use crate::config::Config;
use crate::core::document::Document;
use crate::error::HxResult;

use super::hash_display_range;
use super::outcome::{ExecArtifact, ExecOutcome};
use super::range::ExecRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecSelection {
    pub range: ExecRange,
}

#[derive(Debug)]
pub struct ExecSession {
    pub document: Document,
    pub cursor: u64,
    pub selection: Option<ExecSelection>,
    pub config: Config,
}

impl ExecSession {
    pub fn open(path: &Path, config: Config) -> HxResult<Self> {
        let mut document = Document::open(path, &config)?;
        let cursor = initial_cursor(&mut document, config.initial_offset)?;
        Ok(Self {
            document,
            cursor,
            selection: None,
            config,
        })
    }

    pub fn from_document(document: Document, config: Config) -> Self {
        let cursor = if document.is_empty() {
            0
        } else {
            config.initial_offset.min(document.len() - 1)
        };
        Self {
            document,
            cursor,
            selection: None,
            config,
        }
    }

    pub fn goto(&mut self, offset: u64) -> HxResult<ExecOutcome> {
        self.cursor = self.document.goto(offset)?;
        let mut outcome = ExecOutcome::new(format!("moved to display 0x{:x}", self.cursor), false);
        outcome.cursor = Some(self.cursor);
        Ok(outcome)
    }

    pub fn select(&mut self, range: ExecRange) -> HxResult<ExecOutcome> {
        range.display_bounds(&self.document)?;
        self.selection = Some(ExecSelection { range });
        let mut outcome = ExecOutcome::new("selected range", false);
        outcome.selection = Some(range);
        Ok(outcome)
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn active_display_bounds(&self) -> HxResult<Option<(u64, u64)>> {
        self.selection
            .map(|selection| selection.range.display_bounds(&self.document))
            .transpose()
            .map(Option::flatten)
    }

    pub fn read(&mut self, range: ExecRange) -> HxResult<ExecOutcome> {
        let Some((start, end)) = range.display_bounds(&self.document)? else {
            let mut outcome = ExecOutcome::new("read 0 bytes", false);
            outcome.bytes_read = Some(0);
            outcome.artifacts.push(ExecArtifact::Bytes(Vec::new()));
            return Ok(outcome);
        };
        let bytes = self.document.logical_bytes(start, end)?;
        let mut outcome = ExecOutcome::new(format!("read {} logical bytes", bytes.len()), false);
        outcome.bytes_read = Some(bytes.len() as u64);
        outcome.artifacts.push(ExecArtifact::Bytes(bytes));
        Ok(outcome)
    }

    pub fn hash_active_or_all(&mut self, algorithm: HashAlgorithm) -> HxResult<ExecOutcome> {
        let (start, end, scope) = if let Some((start, end)) = self.active_display_bounds()? {
            (start, end, "selection")
        } else if self.document.is_empty() {
            let mut outcome =
                ExecOutcome::new(format!("{}: no data to hash", algorithm.label()), false);
            outcome.bytes_read = Some(0);
            return Ok(outcome);
        } else {
            (0, self.document.len() - 1, "entire file")
        };

        self.hash_display_range_with_scope(algorithm, start, end, scope)
    }

    pub fn hash(&mut self, algorithm: HashAlgorithm, range: ExecRange) -> HxResult<ExecOutcome> {
        let Some((start, end)) = range.display_bounds(&self.document)? else {
            let mut outcome =
                ExecOutcome::new(format!("{}: no data to hash", algorithm.label()), false);
            outcome.bytes_read = Some(0);
            return Ok(outcome);
        };
        self.hash_display_range_with_scope(algorithm, start, end, "range")
    }

    pub fn search_forward(&mut self, pattern: &[u8], start: Option<u64>) -> HxResult<ExecOutcome> {
        let start = start.unwrap_or(self.cursor);
        let found = self.document.search_forward(start, pattern)?;
        self.search_outcome("search", found)
    }

    pub fn search_backward(
        &mut self,
        pattern: &[u8],
        end_exclusive: Option<u64>,
    ) -> HxResult<ExecOutcome> {
        let end_exclusive = end_exclusive.unwrap_or(self.cursor);
        let found = self.document.search_backward(end_exclusive, pattern)?;
        self.search_outcome("search backward", found)
    }

    pub fn save(&mut self, path: Option<PathBuf>) -> HxResult<ExecOutcome> {
        let (saved, profile) = self.document.save(path)?;
        let mut outcome = ExecOutcome::new(
            format!("wrote {} [{}]", saved.display(), profile),
            self.document.is_dirty(),
        );
        outcome.artifacts.push(ExecArtifact::File(saved));
        Ok(outcome)
    }

    fn hash_display_range_with_scope(
        &mut self,
        algorithm: HashAlgorithm,
        start: u64,
        end: u64,
        scope: &str,
    ) -> HxResult<ExecOutcome> {
        let hash = hash_display_range(&mut self.document, algorithm, start, end)?;
        let bytes_hashed = hash.bytes_hashed;
        if bytes_hashed == 0 {
            let mut outcome =
                ExecOutcome::new(format!("{}: no data to hash", algorithm.label()), false);
            outcome.bytes_read = Some(0);
            return Ok(outcome);
        }

        let mut outcome = ExecOutcome::new(
            format!(
                "{} [{scope} 0x{start:x}-0x{end:x}]: {} ({bytes_hashed} bytes)",
                algorithm.label(),
                hash.hex,
            ),
            false,
        );
        outcome.bytes_read = Some(bytes_hashed);
        outcome.artifacts.push(ExecArtifact::Text(hash.hex));
        Ok(outcome)
    }

    fn search_outcome(&mut self, label: &str, found: Option<u64>) -> HxResult<ExecOutcome> {
        match found {
            Some(offset) => {
                self.cursor = self.document.goto(offset)?;
                let mut outcome = ExecOutcome::new(format!("{label}: display 0x{offset:x}"), false);
                outcome.cursor = Some(offset);
                Ok(outcome)
            }
            None => Ok(ExecOutcome::new(format!("{label}: not found"), false)),
        }
    }
}

fn initial_cursor(document: &mut Document, offset: u64) -> HxResult<u64> {
    if document.is_empty() {
        Ok(0)
    } else {
        document.goto(offset.min(document.len() - 1))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn session_with_bytes(bytes: &[u8]) -> ExecSession {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        ExecSession::open(file.path(), Config::default()).unwrap()
    }

    fn bytes_artifact(outcome: ExecOutcome) -> Vec<u8> {
        match outcome.artifacts.into_iter().next() {
            Some(ExecArtifact::Bytes(bytes)) => bytes,
            other => panic!("expected bytes artifact, got {other:?}"),
        }
    }

    #[test]
    fn save_reports_clean_dirty_state() {
        let mut session = session_with_bytes(b"abcd");
        session.document.replace_display_byte(1, b'Z').unwrap();
        assert!(session.document.is_dirty());

        let outcome = session.save(None).unwrap();

        assert!(!outcome.dirty);
        assert!(!session.document.is_dirty());
    }

    #[test]
    fn logical_read_maps_through_tombstones() {
        let mut session = session_with_bytes(b"abcd");
        session.document.mark_tombstone(1).unwrap();

        let outcome = session.read(ExecRange::logical(1, 2)).unwrap();

        assert_eq!(bytes_artifact(outcome), b"cd");
    }

    #[test]
    fn display_read_skips_tombstones_as_logical_bytes() {
        let mut session = session_with_bytes(b"abcd");
        session.document.mark_tombstone(1).unwrap();

        let outcome = session.read(ExecRange::display(1, 3)).unwrap();

        assert_eq!(bytes_artifact(outcome), b"cd");
    }

    #[test]
    fn search_reports_display_offsets() {
        let mut session = session_with_bytes(b"abcd");
        session.document.mark_tombstone(1).unwrap();

        let outcome = session.search_forward(b"cd", Some(0)).unwrap();

        assert_eq!(outcome.cursor, Some(2));
    }

    #[test]
    fn selection_rejects_out_of_range_logical_span() {
        let mut session = session_with_bytes(b"abcd");
        session.document.mark_tombstone(1).unwrap();

        assert!(session.select(ExecRange::logical(2, 2)).is_err());
    }
}
