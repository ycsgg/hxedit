use std::sync::Arc;

use crate::core::document::{BytesOverlayRun, Document, ReplacementPatch};
use crate::core::piece_table::CellId;
use crate::error::{HxError, HxResult};
use crate::mode::NibblePhase;

use super::walk::WalkControl;

/// A single cell's replacement change: `(cell, before, after)` where each
/// replacement value is `None` when the cell shows its base byte. Returned by
/// the streaming in-place transforms so callers can build an undo record.
pub type ReplacementDelta = (CellId, Option<u8>, Option<u8>);

const REPLACEMENT_CHUNK: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactReplacementStats {
    pub visited: u64,
    pub changed: u64,
}

mod byte;
mod overwrite;
mod replacement;
mod transform;
