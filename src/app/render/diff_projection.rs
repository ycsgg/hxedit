use super::*;

#[derive(Debug, Clone, Copy)]
struct DiffAlignedCell {
    other_offset: Option<u64>,
}

#[derive(Debug, Default)]
struct DiffAlignment {
    current_cells: HashMap<u64, DiffAlignedCell>,
    other_bytes: HashMap<u64, u8>,
    cells: Vec<AlignedDiffCell>,
}

#[derive(Debug, Clone, Copy)]
struct AlignedDiffCell {
    current: Option<DiffByte>,
    other: Option<DiffByte>,
    anchor_display: Option<u64>,
    visual_display: Option<u64>,
    kind: diff_panel::DiffPanelCellKind,
}

#[derive(Debug, Clone)]
struct WindowDiffSource {
    bytes: Vec<DiffByte>,
    index: usize,
}

impl WindowDiffSource {
    fn new(bytes: Vec<DiffByte>) -> Self {
        Self { bytes, index: 0 }
    }
}

impl DiffSource for WindowDiffSource {
    fn read_next(&mut self, max_bytes: usize) -> crate::error::HxResult<Vec<DiffByte>> {
        if max_bytes == 0 || self.index >= self.bytes.len() {
            return Ok(Vec::new());
        }
        let end = (self.index + max_bytes).min(self.bytes.len());
        let out = self.bytes[self.index..end].to_vec();
        self.index = end;
        Ok(out)
    }
}

const DIFF_RENDER_MAX_SHIFT: usize = 4096;
const DIFF_RENDER_EXTRA_CONTEXT: usize = 64;

fn visible_display_bounds(visible_rows: &VisibleRows) -> Option<(u64, u64)> {
    let start = visible_rows.offsets.first().copied()?;
    let end = visible_rows
        .offsets
        .last()
        .zip(visible_rows.rows.last())
        .map(|(offset, row)| offset.saturating_add(row.len().saturating_sub(1) as u64))?;
    Some((start, end))
}

fn build_diff_alignment(
    current: &[DiffByte],
    other: &[DiffByte],
    result: &DiffResult,
) -> DiffAlignment {
    let mut alignment = DiffAlignment {
        current_cells: HashMap::new(),
        other_bytes: other
            .iter()
            .map(|byte| (byte.stream_offset, byte.byte))
            .collect(),
        cells: Vec::new(),
    };
    let Some(first_current) = current.first() else {
        return alignment;
    };
    let current_end = current
        .last()
        .map(|byte| byte.stream_offset.saturating_add(1))
        .unwrap_or(first_current.stream_offset);
    let mut current_pos = first_current.stream_offset;
    let mut other_pos = other
        .first()
        .map(|byte| byte.stream_offset)
        .unwrap_or_default();

    for hunk in &result.hunks {
        map_equal_alignment(
            &mut alignment,
            current,
            other,
            current_pos,
            hunk.current.logical_start,
            other_pos,
        );
        match hunk.kind {
            DiffHunkKind::OnlyCurrent => {
                map_only_current_alignment(
                    &mut alignment,
                    current,
                    hunk.current.logical_start,
                    hunk.current.logical_len,
                );
            }
            DiffHunkKind::OnlyOther => {
                map_only_other_alignment(
                    &mut alignment,
                    current,
                    other,
                    hunk.current.logical_start,
                    hunk.other.offset,
                    hunk.other.len,
                );
            }
            DiffHunkKind::Replace | DiffHunkKind::Unresolved => {
                map_replace_alignment(
                    &mut alignment,
                    current,
                    other,
                    hunk.current.logical_start,
                    hunk.current.logical_len,
                    hunk.other.offset,
                    hunk.other.len,
                );
            }
        }
        current_pos = hunk
            .current
            .logical_start
            .saturating_add(hunk.current.logical_len);
        other_pos = hunk.other.offset.saturating_add(hunk.other.len);
    }
    map_equal_alignment(
        &mut alignment,
        current,
        other,
        current_pos,
        current_end,
        other_pos,
    );
    alignment
}

fn map_equal_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    other: &[DiffByte],
    current_start: u64,
    current_end: u64,
    other_start: u64,
) {
    if current_start >= current_end {
        return;
    }
    for current_offset in current_start..current_end {
        let delta = current_offset.saturating_sub(current_start);
        alignment.current_cells.insert(
            current_offset,
            DiffAlignedCell {
                other_offset: Some(other_start.saturating_add(delta)),
            },
        );
        let Some(current_byte) = find_diff_byte(current, current_offset) else {
            continue;
        };
        let other_byte = find_diff_byte(other, other_start.saturating_add(delta));
        let kind = match other_byte {
            Some(other_byte) if other_byte.byte == current_byte.byte => {
                diff_panel::DiffPanelCellKind::Equal
            }
            Some(_) => diff_panel::DiffPanelCellKind::Replace,
            None => diff_panel::DiffPanelCellKind::OnlyCurrent,
        };
        alignment.cells.push(AlignedDiffCell {
            current: Some(current_byte),
            other: other_byte,
            anchor_display: current_byte.display_offset,
            visual_display: current_byte.display_offset,
            kind,
        });
    }
}

fn map_only_current_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    current_start: u64,
    len: u64,
) {
    for delta in 0..len {
        let current_offset = current_start.saturating_add(delta);
        alignment
            .current_cells
            .insert(current_offset, DiffAlignedCell { other_offset: None });
        let Some(current_byte) = find_diff_byte(current, current_offset) else {
            continue;
        };
        alignment.cells.push(AlignedDiffCell {
            current: Some(current_byte),
            other: None,
            anchor_display: current_byte.display_offset,
            visual_display: current_byte.display_offset,
            kind: diff_panel::DiffPanelCellKind::OnlyCurrent,
        });
    }
}

fn map_only_other_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    other: &[DiffByte],
    current_anchor: u64,
    other_start: u64,
    len: u64,
) {
    let anchor_display = anchor_display_for_current(current, current_anchor);
    for delta in 0..len {
        let Some(other_byte) = find_diff_byte(other, other_start.saturating_add(delta)) else {
            continue;
        };
        let visual_display = anchor_display.map(|display| display.saturating_add(delta));
        alignment.cells.push(AlignedDiffCell {
            current: None,
            other: Some(other_byte),
            anchor_display,
            visual_display,
            kind: diff_panel::DiffPanelCellKind::OnlyOther,
        });
    }
}

fn map_replace_alignment(
    alignment: &mut DiffAlignment,
    current: &[DiffByte],
    other: &[DiffByte],
    current_start: u64,
    current_len: u64,
    other_start: u64,
    other_len: u64,
) {
    let shared = current_len.min(other_len);
    for delta in 0..shared {
        alignment.current_cells.insert(
            current_start.saturating_add(delta),
            DiffAlignedCell {
                other_offset: Some(other_start.saturating_add(delta)),
            },
        );
        let Some(current_byte) = find_diff_byte(current, current_start.saturating_add(delta))
        else {
            continue;
        };
        let other_byte = find_diff_byte(other, other_start.saturating_add(delta));
        let kind = match other_byte {
            Some(other_byte) if other_byte.byte == current_byte.byte => {
                diff_panel::DiffPanelCellKind::Equal
            }
            Some(_) => diff_panel::DiffPanelCellKind::Replace,
            None => diff_panel::DiffPanelCellKind::OnlyCurrent,
        };
        alignment.cells.push(AlignedDiffCell {
            current: Some(current_byte),
            other: other_byte,
            anchor_display: current_byte.display_offset,
            visual_display: current_byte.display_offset,
            kind,
        });
    }
    if current_len > shared {
        map_only_current_alignment(
            alignment,
            current,
            current_start.saturating_add(shared),
            current_len - shared,
        );
    }
    if other_len > shared {
        map_only_other_alignment(
            alignment,
            current,
            other,
            current_start.saturating_add(shared),
            other_start.saturating_add(shared),
            other_len - shared,
        );
    }
}

fn find_diff_byte(bytes: &[DiffByte], stream_offset: u64) -> Option<DiffByte> {
    let first = bytes.first()?.stream_offset;
    let idx = stream_offset.checked_sub(first)? as usize;
    bytes
        .get(idx)
        .copied()
        .filter(|byte| byte.stream_offset == stream_offset)
}

fn anchor_display_for_current(current: &[DiffByte], current_anchor: u64) -> Option<u64> {
    find_diff_byte(current, current_anchor)
        .and_then(|byte| byte.display_offset)
        .or_else(|| {
            current
                .iter()
                .rev()
                .find(|byte| byte.stream_offset < current_anchor)
                .and_then(|byte| byte.display_offset)
                .map(|display| display.saturating_add(1))
        })
}

fn aligned_cell_display_offset(cell: &AlignedDiffCell) -> Option<u64> {
    cell.current
        .and_then(|byte| byte.display_offset)
        .or(cell.visual_display)
        .or(cell.anchor_display)
}

fn diff_overlay_kind_for_panel_kind(
    kind: diff_panel::DiffPanelCellKind,
) -> Option<hex_grid::DiffOverlayKind> {
    match kind {
        diff_panel::DiffPanelCellKind::Replace => Some(hex_grid::DiffOverlayKind::Replace),
        diff_panel::DiffPanelCellKind::OnlyCurrent => Some(hex_grid::DiffOverlayKind::OnlyCurrent),
        diff_panel::DiffPanelCellKind::OnlyOther => Some(hex_grid::DiffOverlayKind::OnlyOther),
        diff_panel::DiffPanelCellKind::Equal | diff_panel::DiffPanelCellKind::Gap => None,
    }
}

impl App {
    pub(super) fn visible_diff_page(
        &mut self,
        visible_rows: &VisibleRows,
    ) -> crate::error::HxResult<VisibleDiffPage> {
        if !self.diff_projection_active() || visible_rows.offsets.is_empty() {
            return Ok(VisibleDiffPage::default());
        }

        let alignment = self.diff_alignment_for_visible(visible_rows)?;
        if let Some(alignment) = alignment
            .as_ref()
            .filter(|alignment| !alignment.cells.is_empty())
        {
            return Ok(self.projected_visible_diff_page(visible_rows, alignment));
        }
        let mut page = VisibleDiffPage::default();
        for (row_idx, row) in visible_rows.rows.iter().enumerate() {
            let row_offset = visible_rows
                .offsets
                .get(row_idx)
                .copied()
                .unwrap_or_default();
            let mut cells = Vec::with_capacity(row.len());
            for (col_idx, slot) in row.iter().enumerate() {
                let display_offset = row_offset + col_idx as u64;
                let (other_byte, kind) =
                    self.diff_cell_for_slot(display_offset, *slot, alignment.as_ref())?;
                if matches!(
                    kind,
                    diff_panel::DiffPanelCellKind::Replace
                        | diff_panel::DiffPanelCellKind::OnlyCurrent
                        | diff_panel::DiffPanelCellKind::OnlyOther
                ) {
                    let overlay_kind = match kind {
                        diff_panel::DiffPanelCellKind::Replace => {
                            hex_grid::DiffOverlayKind::Replace
                        }
                        diff_panel::DiffPanelCellKind::OnlyCurrent => {
                            hex_grid::DiffOverlayKind::OnlyCurrent
                        }
                        diff_panel::DiffPanelCellKind::OnlyOther => {
                            hex_grid::DiffOverlayKind::OnlyOther
                        }
                        diff_panel::DiffPanelCellKind::Equal
                        | diff_panel::DiffPanelCellKind::Gap => {
                            unreachable!("only mismatch kinds are overlaid")
                        }
                    };
                    page.overlay_spans.push(hex_grid::DiffOverlaySpan {
                        start: display_offset,
                        end: display_offset,
                        kind: overlay_kind,
                        active: false,
                    });
                }
                let other_offset = if other_byte.is_some() {
                    self.document
                        .logical_offset_for_display_offset(display_offset)
                        .or(Some(display_offset))
                } else {
                    None
                };
                let current_display_offset =
                    matches!(slot, ByteSlot::Present(_)).then_some(display_offset);
                cells.push(diff_panel::DiffPanelCell {
                    other_byte,
                    kind,
                    other_offset,
                    current_display_offset,
                    visual_display_offset: Some(display_offset),
                    active: display_offset == self.cursor,
                });
            }
            page.rows.push(diff_panel::DiffPanelRow {
                display_offset: row_offset,
                cells,
            });
        }
        Ok(page)
    }

    fn projected_visible_diff_page(
        &self,
        visible_rows: &VisibleRows,
        alignment: &DiffAlignment,
    ) -> VisibleDiffPage {
        let Some(visible_start) = visible_rows.offsets.first().copied() else {
            return VisibleDiffPage::default();
        };
        let start_idx = alignment
            .cells
            .iter()
            .position(|cell| {
                cell.anchor_display
                    .map(|display| display >= visible_start)
                    .unwrap_or(false)
            })
            .unwrap_or(alignment.cells.len());
        let mut idx = start_idx;
        let mut page = VisibleDiffPage::default();
        let bytes_per_line = self.config.bytes_per_line;

        for row_idx in 0..visible_rows.rows.len() {
            let fallback_offset = visible_rows
                .offsets
                .get(row_idx)
                .copied()
                .unwrap_or(visible_start);
            let row_offset = alignment
                .cells
                .get(idx)
                .and_then(aligned_cell_display_offset)
                .unwrap_or(fallback_offset);
            page.main_row_offsets.push(row_offset);

            let mut panel_cells = Vec::with_capacity(bytes_per_line);
            let mut main_cells = Vec::with_capacity(bytes_per_line);
            let mut ascii_cells = Vec::with_capacity(bytes_per_line);
            for _ in 0..bytes_per_line {
                let Some(cell) = alignment.cells.get(idx).copied() else {
                    panel_cells.push(diff_panel::DiffPanelCell {
                        other_byte: None,
                        kind: diff_panel::DiffPanelCellKind::Gap,
                        other_offset: None,
                        current_display_offset: None,
                        visual_display_offset: None,
                        active: false,
                    });
                    main_cells.push(hex_grid::HexGridCell {
                        slot: ByteSlot::Empty,
                        display_offset: None,
                        diff: None,
                        visual_offset: None,
                        other_offset: None,
                    });
                    ascii_cells.push((ByteSlot::Empty, None, false));
                    continue;
                };
                idx += 1;
                let slot = cell
                    .current
                    .map(|byte| ByteSlot::Present(byte.byte))
                    .unwrap_or(ByteSlot::Empty);
                let display_offset = cell.current.and_then(|byte| byte.display_offset);
                let visual_offset = cell
                    .current
                    .and_then(|byte| byte.display_offset)
                    .or(cell.visual_display)
                    .or(cell.anchor_display);
                let diff = diff_overlay_kind_for_panel_kind(cell.kind);
                panel_cells.push(diff_panel::DiffPanelCell {
                    other_byte: cell.other.map(|byte| byte.byte),
                    kind: cell.kind,
                    other_offset: cell.other.map(|byte| byte.stream_offset),
                    current_display_offset: display_offset,
                    visual_display_offset: visual_offset,
                    active: self.diff_aligned_cell_is_active(cell),
                });
                main_cells.push(hex_grid::HexGridCell {
                    slot,
                    display_offset,
                    diff,
                    visual_offset,
                    other_offset: cell.other.map(|byte| byte.stream_offset),
                });
                ascii_cells.push((
                    slot,
                    display_offset,
                    cell.kind == diff_panel::DiffPanelCellKind::OnlyOther,
                ));
            }
            page.rows.push(diff_panel::DiffPanelRow {
                display_offset: row_offset,
                cells: panel_cells,
            });
            page.main_rows.push(main_cells);
            page.main_ascii_rows.push(ascii_cells);
        }

        page
    }

    fn diff_alignment_for_visible(
        &mut self,
        visible_rows: &VisibleRows,
    ) -> crate::error::HxResult<Option<DiffAlignment>> {
        if !self.diff_projection_active() {
            return Ok(None);
        }
        let Some(state) = self.diff_state() else {
            return Ok(None);
        };
        let options = state.options;
        if options.max_shift == 0 || self.document.is_empty() {
            return Ok(None);
        }
        let Some((visible_start, visible_end)) = visible_display_bounds(visible_rows) else {
            return Ok(None);
        };

        let render_shift = options.max_shift.min(DIFF_RENDER_MAX_SHIFT);
        if render_shift == 0 {
            return Ok(None);
        }
        let context = render_shift
            .saturating_add(options.anchor_len)
            .saturating_add(options.verify_len)
            .saturating_add(DIFF_RENDER_EXTRA_CONTEXT);
        let display_start = visible_start.saturating_sub(context as u64);
        let display_end = visible_end
            .saturating_add(context as u64)
            .min(self.document.len().saturating_sub(1));
        let current = self.collect_diff_window_bytes(display_start, display_end)?;
        if current.is_empty() {
            return Ok(Some(DiffAlignment::default()));
        }

        let current_first = current
            .first()
            .map(|byte| byte.stream_offset)
            .unwrap_or_default();
        let current_last = current
            .last()
            .map(|byte| byte.stream_offset)
            .unwrap_or(current_first);
        let other_start = current_first.saturating_sub(render_shift as u64);
        let other_end = current_last
            .saturating_add(context as u64)
            .saturating_add(render_shift as u64);
        let other = self.collect_other_diff_window_bytes(other_start, other_end)?;
        if other.is_empty() {
            let mut alignment = DiffAlignment::default();
            for byte in &current {
                alignment
                    .current_cells
                    .insert(byte.stream_offset, DiffAlignedCell { other_offset: None });
            }
            return Ok(Some(alignment));
        }

        let mut local_options = options;
        local_options.max_shift = render_shift.saturating_mul(2).max(1);
        local_options.hunk_cap = options.hunk_cap.max(visible_rows.rows.len().max(1) * 4);
        let result = diff_sources(
            WindowDiffSource::new(current.clone()),
            WindowDiffSource::new(other.clone()),
            local_options,
        )?;
        Ok(Some(build_diff_alignment(&current, &other, &result)))
    }

    fn collect_diff_window_bytes(
        &mut self,
        display_start: u64,
        display_end: u64,
    ) -> crate::error::HxResult<Vec<DiffByte>> {
        if self.document.is_empty() || display_start > display_end {
            return Ok(Vec::new());
        }
        let end = display_end.min(self.document.len().saturating_sub(1));
        let mut out = Vec::with_capacity((end - display_start + 1).min(8192) as usize);
        for display_offset in display_start..=end {
            let ByteSlot::Present(byte) = self.document.byte_at(display_offset)? else {
                continue;
            };
            if let Some(logical_offset) = self
                .document
                .logical_offset_for_display_offset(display_offset)
            {
                out.push(DiffByte {
                    stream_offset: logical_offset,
                    display_offset: Some(display_offset),
                    byte,
                });
            }
        }
        Ok(out)
    }

    fn collect_other_diff_window_bytes(
        &mut self,
        other_start: u64,
        other_end: u64,
    ) -> crate::error::HxResult<Vec<DiffByte>> {
        let Some(state) = self.diff_state_mut() else {
            return Ok(Vec::new());
        };
        if other_start > other_end || other_start >= state.other_len {
            return Ok(Vec::new());
        }
        let end = other_end.min(state.other_len.saturating_sub(1));
        let len = (end - other_start + 1) as usize;
        let raw = state.other_view.read_range(other_start, len)?;
        Ok(raw
            .into_iter()
            .enumerate()
            .map(|(idx, byte)| DiffByte {
                stream_offset: other_start + idx as u64,
                display_offset: None,
                byte,
            })
            .collect())
    }

    fn diff_cell_for_slot(
        &mut self,
        display_offset: u64,
        slot: ByteSlot,
        alignment: Option<&DiffAlignment>,
    ) -> crate::error::HxResult<(Option<u8>, diff_panel::DiffPanelCellKind)> {
        let current = match slot {
            ByteSlot::Present(current) => current,
            ByteSlot::Empty => {
                let other = self.read_diff_other_byte(display_offset)?;
                let kind = if other.is_some() {
                    diff_panel::DiffPanelCellKind::OnlyOther
                } else {
                    diff_panel::DiffPanelCellKind::Gap
                };
                return Ok((other, kind));
            }
            ByteSlot::Deleted => return Ok((None, diff_panel::DiffPanelCellKind::Gap)),
        };
        let Some(logical_offset) = self
            .document
            .logical_offset_for_display_offset(display_offset)
        else {
            return Ok((None, diff_panel::DiffPanelCellKind::Gap));
        };
        if let Some(alignment) = alignment {
            if let Some(cell) = alignment.current_cells.get(&logical_offset) {
                let Some(other_offset) = cell.other_offset else {
                    return Ok((None, diff_panel::DiffPanelCellKind::OnlyCurrent));
                };
                let Some(&other_byte) = alignment.other_bytes.get(&other_offset) else {
                    return Ok((None, diff_panel::DiffPanelCellKind::OnlyCurrent));
                };
                let kind = if other_byte == current {
                    diff_panel::DiffPanelCellKind::Equal
                } else {
                    diff_panel::DiffPanelCellKind::Replace
                };
                return Ok((Some(other_byte), kind));
            }
        }
        let other = self.read_diff_other_byte(logical_offset)?;
        let kind = match other {
            Some(byte) if byte == current => diff_panel::DiffPanelCellKind::Equal,
            Some(_) => diff_panel::DiffPanelCellKind::Replace,
            None => diff_panel::DiffPanelCellKind::OnlyCurrent,
        };
        Ok((other, kind))
    }

    fn diff_aligned_cell_is_active(&self, cell: AlignedDiffCell) -> bool {
        match cell.kind {
            diff_panel::DiffPanelCellKind::OnlyOther => {
                let Some(state) = self.diff_state() else {
                    return false;
                };
                let Some(other_offset) = cell.other.map(|byte| byte.stream_offset) else {
                    return false;
                };
                state.selected_other_offset == Some(other_offset)
                    && state
                        .selected_other_anchor_display
                        .is_some_and(|anchor| anchor == self.cursor)
            }
            _ => cell
                .current
                .and_then(|byte| byte.display_offset)
                .is_some_and(|display| display == self.cursor),
        }
    }

    pub(crate) fn visible_diff_cell_hit(
        &mut self,
        visible_row: usize,
        col: usize,
        side: crate::input::mouse::DiffCellSide,
    ) -> Option<DiffCellHit> {
        if !self.diff_projection_active() || col >= self.config.bytes_per_line {
            return None;
        }
        let visible_rows = self.collect_visible_rows(self.view_rows);
        let page = self.visible_diff_page(&visible_rows).ok()?;
        let row = page.rows.get(visible_row)?;
        let cell = row.cells.get(col)?;
        if matches!(cell.kind, diff_panel::DiffPanelCellKind::Gap) {
            return None;
        }
        match side {
            crate::input::mouse::DiffCellSide::Current => {
                let visual_offset = cell
                    .visual_display_offset
                    .or(cell.current_display_offset)
                    .or_else(|| page.main_rows.get(visible_row)?.get(col)?.visual_offset)?;
                Some(DiffCellHit {
                    side,
                    visual_offset,
                    current_display_offset: cell.current_display_offset,
                    other_offset: cell.other_offset,
                })
            }
            crate::input::mouse::DiffCellSide::Other => {
                let other_offset = cell.other_offset?;
                let visual_offset = cell
                    .visual_display_offset
                    .or(cell.current_display_offset)
                    .or_else(|| page.main_rows.get(visible_row)?.get(col)?.visual_offset)
                    .unwrap_or(row.display_offset.saturating_add(col as u64));
                Some(DiffCellHit {
                    side,
                    visual_offset,
                    current_display_offset: cell.current_display_offset,
                    other_offset: Some(other_offset),
                })
            }
        }
    }
}
