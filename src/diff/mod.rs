//! Streaming diff support.
//!
//! The diff subsystem compares the current document's logical byte stream with
//! a second raw byte stream. It is intentionally read-only: it never mutates the
//! document and never participates in undo/save semantics.

mod engine;
mod source;

pub use engine::{
    diff_sources, CurrentDiffRange, DiffHunk, DiffHunkKind, DiffOptions, DiffProfile, DiffResult,
    DiffStatus, DiffTruncateReason, OtherDiffRange,
};
pub use source::{DiffByte, DiffSource, DocumentLogicalCursor, FileDiffSource, VecDiffSource};
