use crate::core::document::Document;
use crate::error::{HxError, HxResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSpace {
    Display,
    Logical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecRange {
    pub space: RangeSpace,
    pub start: u64,
    pub len: u64,
}

impl ExecRange {
    pub fn display(start: u64, len: u64) -> Self {
        Self {
            space: RangeSpace::Display,
            start,
            len,
        }
    }

    pub fn logical(start: u64, len: u64) -> Self {
        Self {
            space: RangeSpace::Logical,
            start,
            len,
        }
    }

    pub fn end_exclusive(self) -> HxResult<u64> {
        self.start
            .checked_add(self.len)
            .ok_or(HxError::OffsetOutOfRange)
    }

    pub fn display_bounds(self, document: &Document) -> HxResult<Option<(u64, u64)>> {
        if self.len == 0 || document.is_empty() {
            return Ok(None);
        }

        match self.space {
            RangeSpace::Display => {
                let end_exclusive = self.end_exclusive()?;
                if self.start >= document.len() || end_exclusive > document.len() {
                    return Err(HxError::OffsetOutOfRange);
                }
                Ok(Some((self.start, end_exclusive - 1)))
            }
            RangeSpace::Logical => {
                let end_exclusive = self.end_exclusive()?;
                if self.start >= document.visible_len() || end_exclusive > document.visible_len() {
                    return Err(HxError::OffsetOutOfRange);
                }
                let start = document
                    .display_offset_for_logical_offset(self.start)
                    .ok_or(HxError::OffsetOutOfRange)?;
                let end = document
                    .display_offset_for_logical_offset(end_exclusive - 1)
                    .ok_or(HxError::OffsetOutOfRange)?;
                Ok(Some((start, end)))
            }
        }
    }
}
