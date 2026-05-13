use std::path::Path;

use crate::core::document::Document;
use crate::core::file_view::FileView;
use crate::core::piece_table::{CellId, Piece, PieceSource};
use crate::error::HxResult;

const SOURCE_CHUNK: usize = 64 * 1024;

/// One byte read from either side of the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffByte {
    /// Logical offset for the current document side, raw file offset for the
    /// other side.
    pub stream_offset: u64,
    /// Display offset for current-document bytes. `None` for raw other-side
    /// file bytes.
    pub display_offset: Option<u64>,
    pub byte: u8,
}

/// Streaming byte source used by the diff engine.
pub trait DiffSource {
    /// Return up to `max_bytes` bytes, or an empty vec at EOF.
    fn read_next(&mut self, max_bytes: usize) -> HxResult<Vec<DiffByte>>;
}

/// Streaming cursor over `Document` logical bytes.
///
/// Tombstones are skipped, replacements are applied, and inserted Add bytes are
/// included. Each emitted byte keeps its display offset so current-side hunks can
/// later be highlighted in the hex grid without assuming display continuity.
#[derive(Debug, Clone)]
pub struct DocumentLogicalCursor {
    pieces: Vec<Piece>,
    piece_index: usize,
    piece_display_start: u64,
    piece_local_offset: u64,
    logical_offset: u64,
}

impl DocumentLogicalCursor {
    pub fn new(document: &Document) -> Self {
        Self {
            pieces: document.pieces_snapshot(),
            piece_index: 0,
            piece_display_start: 0,
            piece_local_offset: 0,
            logical_offset: 0,
        }
    }

    pub fn read_next(
        &mut self,
        document: &mut Document,
        max_bytes: usize,
    ) -> HxResult<Vec<DiffByte>> {
        if max_bytes == 0 {
            return Ok(Vec::new());
        }

        let has_tombstones = document.has_tombstones();
        let has_replacements = document.has_replacements();
        let mut out = Vec::with_capacity(max_bytes.min(SOURCE_CHUNK));

        while out.len() < max_bytes && self.piece_index < self.pieces.len() {
            let piece = self.pieces[self.piece_index];
            if self.piece_local_offset >= piece.len {
                self.advance_piece(piece.len);
                continue;
            }

            let remaining_in_piece = piece.len - self.piece_local_offset;
            let want = (max_bytes - out.len())
                .min(SOURCE_CHUNK)
                .min(remaining_in_piece as usize);
            if want == 0 {
                break;
            }

            let source_start = piece.start + self.piece_local_offset;
            let display_start = self.piece_display_start + self.piece_local_offset;
            let raw = match piece.source {
                PieceSource::Original => document.raw_range(source_start, want)?,
                PieceSource::Add => document.add_slice(source_start, want as u64).to_vec(),
            };

            if raw.is_empty() {
                self.piece_local_offset = piece.len;
                continue;
            }

            let raw_len = raw.len();
            let need_overlay = overlay_needed(
                document,
                piece.source,
                source_start,
                raw_len as u64,
                has_tombstones,
                has_replacements,
            );

            if !need_overlay.0 && !need_overlay.1 {
                for (idx, &byte) in raw.iter().enumerate() {
                    out.push(DiffByte {
                        stream_offset: self.logical_offset,
                        display_offset: Some(display_start + idx as u64),
                        byte,
                    });
                    self.logical_offset += 1;
                }
            } else {
                for (idx, &base) in raw.iter().enumerate() {
                    let id = CellId::from_source(piece.source, source_start + idx as u64);
                    if need_overlay.0 && document.is_tombstone(id) {
                        continue;
                    }
                    let byte = if need_overlay.1 {
                        document.replacement_for(id).unwrap_or(base)
                    } else {
                        base
                    };
                    out.push(DiffByte {
                        stream_offset: self.logical_offset,
                        display_offset: Some(display_start + idx as u64),
                        byte,
                    });
                    self.logical_offset += 1;
                }
            }

            self.piece_local_offset += raw_len as u64;
        }

        Ok(out)
    }

    fn advance_piece(&mut self, piece_len: u64) {
        self.piece_display_start += piece_len;
        self.piece_index += 1;
        self.piece_local_offset = 0;
    }
}

fn overlay_needed(
    document: &Document,
    source: PieceSource,
    source_offset: u64,
    len: u64,
    has_tombstones: bool,
    has_replacements: bool,
) -> (bool, bool) {
    if len == 0 {
        return (false, false);
    }
    let lo = CellId::from_source(source, source_offset);
    let hi = CellId::from_source(source, source_offset + len - 1);
    (
        has_tombstones && document.has_tombstone_in_range(lo, hi),
        has_replacements && document.has_replacement_in_range(lo, hi),
    )
}

impl<'a> DiffSource for (&'a mut DocumentLogicalCursor, &'a mut Document) {
    fn read_next(&mut self, max_bytes: usize) -> HxResult<Vec<DiffByte>> {
        self.0.read_next(self.1, max_bytes)
    }
}

/// Raw read-only file source for the other side of a diff.
#[derive(Debug)]
pub struct FileDiffSource {
    view: FileView,
    offset: u64,
}

impl FileDiffSource {
    pub fn open(path: &Path, page_size: usize, cache_pages: usize) -> HxResult<Self> {
        Ok(Self {
            view: FileView::open(path, true, page_size, cache_pages)?,
            offset: 0,
        })
    }

    pub fn len(&self) -> u64 {
        self.view.len()
    }

    pub fn is_empty(&self) -> bool {
        self.view.is_empty()
    }
}

impl DiffSource for FileDiffSource {
    fn read_next(&mut self, max_bytes: usize) -> HxResult<Vec<DiffByte>> {
        if max_bytes == 0 || self.offset >= self.view.len() {
            return Ok(Vec::new());
        }
        let to_read = max_bytes.min((self.view.len() - self.offset) as usize);
        let raw = self.view.read_range(self.offset, to_read)?;
        let start = self.offset;
        self.offset += raw.len() as u64;
        Ok(raw
            .into_iter()
            .enumerate()
            .map(|(idx, byte)| DiffByte {
                stream_offset: start + idx as u64,
                display_offset: None,
                byte,
            })
            .collect())
    }
}

/// Test/utility source backed by a byte vector.
#[derive(Debug, Clone)]
pub struct VecDiffSource {
    bytes: Vec<u8>,
    index: usize,
    display_base: Option<u64>,
}

impl VecDiffSource {
    pub fn current(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            index: 0,
            display_base: Some(0),
        }
    }

    pub fn other(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            index: 0,
            display_base: None,
        }
    }

    pub fn current_with_display(bytes: impl Into<Vec<u8>>, display_base: u64) -> Self {
        Self {
            bytes: bytes.into(),
            index: 0,
            display_base: Some(display_base),
        }
    }
}

impl DiffSource for VecDiffSource {
    fn read_next(&mut self, max_bytes: usize) -> HxResult<Vec<DiffByte>> {
        if max_bytes == 0 || self.index >= self.bytes.len() {
            return Ok(Vec::new());
        }
        let end = (self.index + max_bytes).min(self.bytes.len());
        let start = self.index;
        self.index = end;
        Ok(self.bytes[start..end]
            .iter()
            .copied()
            .enumerate()
            .map(|(idx, byte)| DiffByte {
                stream_offset: (start + idx) as u64,
                display_offset: self.display_base.map(|base| base + (start + idx) as u64),
                byte,
            })
            .collect())
    }
}
