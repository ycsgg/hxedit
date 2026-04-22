use crate::app::{App, MainView};
use crate::disasm::regions::{data_row_offsets_before, data_row_start, visible_regions};
use crate::disasm::DisassemblyState;
use crate::executable::CodeSpan;
use crate::mode::Mode;

impl App {
    pub(super) fn move_vertical_disassembly(&mut self, rows: i64) {
        if rows == 0 {
            return;
        }
        match self.disassembly_shift_row_start(self.cursor_anchor_offset(), rows) {
            Ok(Some(target)) => {
                self.cursor = self.clamp_cursor_for_mode(target, self.mode);
            }
            Ok(None) => {}
            Err(err) => {
                self.set_error_status(err.to_string());
                return;
            }
        }
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = crate::mode::NibblePhase::High;
        }
    }

    pub(super) fn ensure_cursor_visible_disassembly(&mut self) -> crate::error::HxResult<()> {
        let Some(state) = self.current_disassembly_state() else {
            return Ok(());
        };
        let Some(cursor_row) =
            self.disassembly_row_start_at_or_after(&state, self.cursor_anchor_offset())?
        else {
            return Ok(());
        };
        let rows = self.collect_disassembly_rows(
            &state,
            state.viewport_top,
            self.visible_rows().max(1) as usize,
        )?;
        if rows.is_empty() {
            return Ok(());
        }
        let first = rows.first().map(|row| row.offset).unwrap_or(cursor_row);
        let last = rows.last().map(|row| row.offset).unwrap_or(cursor_row);
        if cursor_row < first {
            self.set_disassembly_viewport_top(cursor_row);
        } else if cursor_row > last {
            let center_back = self.visible_rows().saturating_sub(1) as usize;
            let top = self
                .disassembly_shift_row_start(cursor_row, -(center_back as i64))?
                .unwrap_or(cursor_row);
            self.set_disassembly_viewport_top(top);
        }
        Ok(())
    }

    pub(crate) fn focus_disassembly_row_at_offset(
        &mut self,
        offset: u64,
    ) -> crate::error::HxResult<()> {
        let Some(state) = self.current_disassembly_state() else {
            return Ok(());
        };

        let target = if let Some(row_start) =
            self.disassembly_row_start_containing(&state, offset)?
        {
            row_start
        } else if let Some(row_start) = self.disassembly_row_start_at_or_after(&state, offset)? {
            row_start
        } else {
            return Ok(());
        };
        self.set_disassembly_viewport_top(target);
        Ok(())
    }

    pub(super) fn center_cursor_in_view_disassembly(&mut self) -> crate::error::HxResult<()> {
        let Some(state) = self.current_disassembly_state() else {
            return Ok(());
        };
        let Some(cursor_row) =
            self.disassembly_row_start_at_or_after(&state, self.cursor_anchor_offset())?
        else {
            return Ok(());
        };
        let center_back = (self.visible_rows() / 2) as i64;
        let top = self
            .disassembly_shift_row_start(cursor_row, -center_back)?
            .unwrap_or(cursor_row);
        self.set_disassembly_viewport_top(top);
        Ok(())
    }

    pub(super) fn scroll_viewport_disassembly(&mut self, rows: i64) -> crate::error::HxResult<()> {
        if rows == 0 {
            return Ok(());
        }
        let Some(current_top) = self.current_disassembly_viewport_top() else {
            return Ok(());
        };
        let Some(target) = self.disassembly_shift_row_start(current_top, rows)? else {
            return Ok(());
        };
        self.set_disassembly_viewport_top(target);
        self.clamp_cursor_into_view_disassembly()
    }

    pub(super) fn clamp_cursor_into_view_disassembly(&mut self) -> crate::error::HxResult<()> {
        let Some(state) = self.current_disassembly_state() else {
            self.cursor = 0;
            return Ok(());
        };
        let rows = self.collect_disassembly_rows(
            &state,
            state.viewport_top,
            self.visible_rows().max(1) as usize,
        )?;
        let Some(first) = rows.first() else {
            return Ok(());
        };
        let Some(last) = rows.last() else {
            return Ok(());
        };
        let visible_start = first.offset;
        let visible_end = last.offset + last.len() as u64 - 1;
        if self.cursor_anchor_offset() < visible_start {
            self.cursor = self.clamp_cursor_for_mode(visible_start, self.mode);
        } else if self.cursor_anchor_offset() > visible_end {
            self.cursor = self.clamp_cursor_for_mode(last.offset, self.mode);
        }
        Ok(())
    }

    fn current_disassembly_state(&self) -> Option<DisassemblyState> {
        match &self.main_view {
            MainView::Disassembly(state) => Some(state.clone()),
            MainView::Hex => None,
        }
    }

    fn current_disassembly_viewport_top(&self) -> Option<u64> {
        self.current_disassembly_state()
            .map(|state| state.viewport_top)
    }

    fn set_disassembly_viewport_top(&mut self, top: u64) {
        if let MainView::Disassembly(state) = &mut self.main_view {
            state.viewport_top = top;
        }
    }

    fn disassembly_shift_row_start(
        &mut self,
        offset: u64,
        rows: i64,
    ) -> crate::error::HxResult<Option<u64>> {
        let Some(state) = self.current_disassembly_state() else {
            return Ok(None);
        };
        if rows >= 0 {
            let Some(base) = self.disassembly_row_start_at_or_after(&state, offset)? else {
                return Ok(None);
            };
            let decoded = self.collect_disassembly_rows(&state, base, rows as usize + 1)?;
            return Ok(decoded
                .get(rows as usize)
                .or_else(|| decoded.last())
                .map(|row| row.offset));
        }

        let limit_exclusive =
            if let Some(containing) = self.disassembly_row_start_containing(&state, offset)? {
                containing
            } else {
                offset.saturating_add(1)
            };
        self.disassembly_previous_row_start_before(
            &state,
            limit_exclusive,
            rows.unsigned_abs() as usize,
        )
    }

    fn disassembly_row_start_at_or_after(
        &mut self,
        state: &DisassemblyState,
        offset: u64,
    ) -> crate::error::HxResult<Option<u64>> {
        Ok(self
            .collect_disassembly_rows(state, offset, 1)?
            .into_iter()
            .next()
            .map(|row| row.offset))
    }

    fn disassembly_row_start_containing(
        &mut self,
        state: &DisassemblyState,
        offset: u64,
    ) -> crate::error::HxResult<Option<u64>> {
        let Some(region) = self.disassembly_visible_region_containing(state, offset) else {
            return Ok(None);
        };
        if !region.executable {
            return Ok(Some(data_row_start(region.start, offset)));
        }
        let mut start = region.start;
        loop {
            let rows = self.collect_disassembly_rows(state, start, 256)?;
            if rows.is_empty() {
                return Ok(None);
            }
            for row in rows {
                if row.offset > region.end_inclusive {
                    return Ok(None);
                }
                let row_end = row.offset + row.len() as u64 - 1;
                if offset >= row.offset && offset <= row_end {
                    return Ok(Some(row.offset));
                }
                if row.offset > offset {
                    return Ok(None);
                }
                start = row_end.saturating_add(1);
            }
        }
    }

    fn disassembly_previous_row_start_before(
        &mut self,
        state: &DisassemblyState,
        limit_exclusive: u64,
        steps: usize,
    ) -> crate::error::HxResult<Option<u64>> {
        if steps == 0 {
            return Ok(Some(limit_exclusive));
        }

        let mut remaining = steps;
        let mut earliest = None;
        let regions = self.disassembly_visible_regions(state);
        for region in regions.into_iter().rev() {
            if region.start >= limit_exclusive {
                continue;
            }
            let region_limit = limit_exclusive.min(region.end_inclusive.saturating_add(1));
            let offsets =
                self.disassembly_row_offsets_in_region_before(state, &region, region_limit)?;
            if let Some(first) = offsets.first().copied() {
                earliest = Some(first);
            }
            if offsets.len() >= remaining {
                return Ok(offsets
                    .get(offsets.len().saturating_sub(remaining))
                    .copied());
            }
            remaining = remaining.saturating_sub(offsets.len());
        }

        Ok(earliest)
    }

    fn disassembly_row_offsets_in_region_before(
        &mut self,
        state: &DisassemblyState,
        region: &CodeSpan,
        limit_exclusive: u64,
    ) -> crate::error::HxResult<Vec<u64>> {
        if !region.executable {
            return Ok(data_row_offsets_before(
                region.start,
                region.end_inclusive,
                limit_exclusive,
            ));
        }

        let mut offsets = Vec::new();
        let mut start = region.start;
        while start <= region.end_inclusive {
            let rows = self.collect_disassembly_rows(state, start, 256)?;
            if rows.is_empty() {
                break;
            }
            let mut next_start = None;
            for row in rows {
                if row.offset > region.end_inclusive || row.offset >= limit_exclusive {
                    return Ok(offsets);
                }
                next_start = Some(row.offset + row.len() as u64);
                offsets.push(row.offset);
            }
            let Some(candidate) = next_start else {
                break;
            };
            if candidate <= start {
                break;
            }
            start = candidate;
        }
        Ok(offsets)
    }

    fn disassembly_visible_region_containing(
        &self,
        state: &DisassemblyState,
        offset: u64,
    ) -> Option<CodeSpan> {
        self.disassembly_visible_regions(state)
            .into_iter()
            .find(|region| region.contains(offset))
    }

    fn disassembly_visible_regions(&self, state: &DisassemblyState) -> Vec<CodeSpan> {
        visible_regions(&state.info, self.document.len())
    }
}
