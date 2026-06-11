use std::path::Path;

use crate::core::document::Document;
use crate::core::file_view::FileView;
use crate::error::HxResult;

use crate::core::document::walk::WalkControl;

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
    display_offset: u64,
    logical_offset: u64,
}

impl DocumentLogicalCursor {
    pub fn new(_document: &Document) -> Self {
        Self {
            display_offset: 0,
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
        let mut out = Vec::with_capacity(max_bytes.min(SOURCE_CHUNK));
        if self.display_offset >= document.len() {
            return Ok(out);
        }

        let start = self.display_offset;
        document.walk_visible_cells(
            start,
            document.len().saturating_sub(1),
            max_bytes.min(SOURCE_CHUNK),
            |_, chunk| {
                for cell in chunk.cells {
                    self.display_offset = cell.display_offset.saturating_add(1);
                    if cell.deleted {
                        continue;
                    }
                    out.push(DiffByte {
                        stream_offset: self.logical_offset,
                        display_offset: Some(cell.display_offset),
                        byte: cell.byte,
                    });
                    self.logical_offset += 1;
                    if out.len() >= max_bytes {
                        return Ok(WalkControl::Stop);
                    }
                }
                Ok(WalkControl::Continue)
            },
        )?;
        Ok(out)
    }
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
