use crate::core::document::walk::WalkControl;
use crate::core::document::Document;
use crate::core::file_view::FileView;
use crate::error::HxResult;

const DIFF_MISMATCH_CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MismatchScanStep {
    pub found: Option<u64>,
    pub next: Option<u64>,
    pub scanned: u64,
}

pub fn find_mismatch_forward(
    document: &mut Document,
    other_view: &mut FileView,
    other_len: u64,
    start: u64,
) -> HxResult<Option<u64>> {
    let doc_len = document.len();
    if start >= doc_len {
        return Ok(None);
    }
    Ok(find_mismatch_forward_step(
        document,
        other_view,
        other_len,
        start,
        doc_len - 1,
        u64::MAX,
    )?
    .found)
}

pub fn find_mismatch_forward_step(
    document: &mut Document,
    other_view: &mut FileView,
    other_len: u64,
    start: u64,
    end_inclusive: u64,
    max_display_bytes: u64,
) -> HxResult<MismatchScanStep> {
    if start > end_inclusive || max_display_bytes == 0 {
        return Ok(MismatchScanStep {
            found: None,
            next: None,
            scanned: 0,
        });
    }
    let doc_len = document.len();
    if start >= doc_len {
        return Ok(MismatchScanStep {
            found: None,
            next: None,
            scanned: 0,
        });
    }
    let end_inclusive = end_inclusive.min(doc_len - 1);
    let step_end = start
        .saturating_add(max_display_bytes.saturating_sub(1))
        .min(end_inclusive);

    let mut found = None;
    let mut next_logical_offset = if document.has_tombstones() {
        None
    } else {
        Some(start)
    };

    document.walk_visible_cells(start, step_end, DIFF_MISMATCH_CHUNK, |document, chunk| {
        if chunk.fast_path {
            let Some(logical_start) = next_logical_offset
                .or_else(|| document.logical_offset_for_display_offset(chunk.display_start))
            else {
                return Ok(WalkControl::Continue);
            };
            let other =
                read_other_range(other_view, other_len, logical_start, chunk.raw_bytes.len())?;
            if let Some(index) = first_forward_diff_index(chunk.raw_bytes, &other) {
                found = Some(chunk.display_start + index as u64);
                return Ok(WalkControl::Stop);
            }
            next_logical_offset = Some(logical_start + chunk.raw_bytes.len() as u64);
            return Ok(WalkControl::Continue);
        }

        let mut display_offsets = Vec::with_capacity(chunk.cells.len());
        let mut current = Vec::with_capacity(chunk.cells.len());
        for cell in chunk.cells {
            if cell.deleted {
                continue;
            }
            display_offsets.push(cell.display_offset);
            current.push(cell.byte);
        }
        if current.is_empty() {
            return Ok(WalkControl::Continue);
        }

        let Some(logical_start) = next_logical_offset
            .or_else(|| document.logical_offset_for_display_offset(display_offsets[0]))
        else {
            return Ok(WalkControl::Continue);
        };
        let other = read_other_range(other_view, other_len, logical_start, current.len())?;
        if let Some(index) = first_forward_diff_index(&current, &other) {
            found = Some(display_offsets[index]);
            return Ok(WalkControl::Stop);
        }
        next_logical_offset = Some(logical_start + current.len() as u64);
        Ok(WalkControl::Continue)
    })?;

    Ok(MismatchScanStep {
        found,
        next: (found.is_none() && step_end < end_inclusive).then_some(step_end + 1),
        scanned: step_end - start + 1,
    })
}

pub fn find_mismatch_backward(
    document: &mut Document,
    other_view: &mut FileView,
    other_len: u64,
    start: u64,
) -> HxResult<Option<u64>> {
    if document.is_empty() {
        return Ok(None);
    }
    Ok(find_mismatch_backward_step(
        document,
        other_view,
        other_len,
        0,
        start.min(document.len() - 1),
        u64::MAX,
    )?
    .found)
}

pub fn find_mismatch_backward_step(
    document: &mut Document,
    other_view: &mut FileView,
    other_len: u64,
    range_start: u64,
    end_inclusive: u64,
    max_display_bytes: u64,
) -> HxResult<MismatchScanStep> {
    if document.is_empty() || range_start > end_inclusive || max_display_bytes == 0 {
        return Ok(MismatchScanStep {
            found: None,
            next: None,
            scanned: 0,
        });
    }
    let end = end_inclusive.min(document.len() - 1);
    let step_start = end
        .saturating_sub(max_display_bytes.saturating_sub(1))
        .max(range_start);
    let mut found = None;

    document.walk_visible_cells_reverse(
        step_start,
        end,
        DIFF_MISMATCH_CHUNK,
        |document, chunk| {
            if chunk.fast_path {
                let Some(logical_start) =
                    document.logical_offset_for_display_offset(chunk.display_start)
                else {
                    return Ok(WalkControl::Continue);
                };
                let other =
                    read_other_range(other_view, other_len, logical_start, chunk.raw_bytes.len())?;
                if let Some(index) = first_backward_diff_index(chunk.raw_bytes, &other) {
                    found = Some(chunk.display_start + index as u64);
                    return Ok(WalkControl::Stop);
                }
                return Ok(WalkControl::Continue);
            }

            let mut display_offsets = Vec::with_capacity(chunk.cells.len());
            let mut current = Vec::with_capacity(chunk.cells.len());
            for cell in chunk.cells {
                if cell.deleted {
                    continue;
                }
                display_offsets.push(cell.display_offset);
                current.push(cell.byte);
            }
            if current.is_empty() {
                return Ok(WalkControl::Continue);
            }

            let Some(logical_start) =
                document.logical_offset_for_display_offset(display_offsets[0])
            else {
                return Ok(WalkControl::Continue);
            };
            let other = read_other_range(other_view, other_len, logical_start, current.len())?;
            if let Some(index) = first_backward_diff_index(&current, &other) {
                found = Some(display_offsets[index]);
                return Ok(WalkControl::Stop);
            }
            Ok(WalkControl::Continue)
        },
    )?;

    Ok(MismatchScanStep {
        found,
        next: (found.is_none() && step_start > range_start).then_some(step_start - 1),
        scanned: end - step_start + 1,
    })
}

fn read_other_range(
    other_view: &mut FileView,
    other_len: u64,
    logical_start: u64,
    len: usize,
) -> HxResult<Vec<u8>> {
    if len == 0 || logical_start >= other_len {
        return Ok(Vec::new());
    }
    let available = (other_len - logical_start).min(len as u64) as usize;
    other_view.read_range(logical_start, available)
}

fn first_forward_diff_index(current: &[u8], other: &[u8]) -> Option<usize> {
    let shared = current.len().min(other.len());
    for index in 0..shared {
        if current[index] != other[index] {
            return Some(index);
        }
    }
    (current.len() > other.len()).then_some(other.len())
}

fn first_backward_diff_index(current: &[u8], other: &[u8]) -> Option<usize> {
    if current.len() > other.len() {
        return current.len().checked_sub(1);
    }
    (0..current.len())
        .rev()
        .find(|&index| current[index] != other[index])
}
