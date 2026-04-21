use crate::app::App;
use crate::mode::Mode;

impl App {
    pub(crate) fn move_horizontal(&mut self, delta: i64) {
        self.ensure_insert_pending_committed();
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = crate::mode::NibblePhase::High;
        }
    }

    pub(crate) fn move_vertical(&mut self, rows: i64) {
        self.ensure_insert_pending_committed();
        if matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            self.move_vertical_disassembly(rows);
            return;
        }
        let delta = rows.saturating_mul(self.config.bytes_per_line as i64);
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = crate::mode::NibblePhase::High;
        }
    }

    fn move_vertical_disassembly(&mut self, rows: i64) {
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

    pub(crate) fn move_row_edge(&mut self, end: bool) {
        self.ensure_insert_pending_committed();
        let row_start = align_offset(self.cursor, self.config.bytes_per_line);
        let target = if end {
            row_start + self.config.bytes_per_line.saturating_sub(1) as u64
        } else {
            row_start
        };
        self.cursor = self.clamp_cursor_for_mode(target, self.mode);
    }

    pub(crate) fn ensure_cursor_visible(&mut self) {
        if matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            if let Err(err) = self.ensure_cursor_visible_disassembly() {
                self.set_error_status(err.to_string());
            }
            return;
        }
        let row_size = self.config.bytes_per_line as u64;
        let cursor_row = align_offset(self.cursor_anchor_offset(), self.config.bytes_per_line);
        let visible_rows = self.visible_rows();
        let bottom = self.viewport_top + visible_rows.saturating_sub(1) * row_size;
        if cursor_row < self.viewport_top {
            self.viewport_top = cursor_row;
        } else if cursor_row > bottom {
            self.viewport_top =
                cursor_row.saturating_sub((visible_rows.saturating_sub(1)) * row_size);
        }
        self.viewport_top = align_offset(self.viewport_top, self.config.bytes_per_line);
    }

    pub(crate) fn center_cursor_in_view(&mut self) {
        if matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            if let Err(err) = self.center_cursor_in_view_disassembly() {
                self.set_error_status(err.to_string());
            }
            return;
        }
        if self.document.is_empty() {
            self.viewport_top = 0;
            return;
        }
        let row_size = self.config.bytes_per_line as u64;
        let cursor_row = align_offset(self.cursor_anchor_offset(), self.config.bytes_per_line);
        let visible_rows = self.visible_rows();
        let bottom = self.viewport_top + visible_rows.saturating_sub(1) * row_size;
        if cursor_row >= self.viewport_top && cursor_row <= bottom {
            return;
        }
        let center_rows = visible_rows / 2;
        let max_top = self.max_viewport_top();
        self.viewport_top = align_offset(
            cursor_row.saturating_sub(center_rows.saturating_mul(row_size)),
            self.config.bytes_per_line,
        )
        .min(max_top);
    }

    pub(crate) fn scroll_viewport(&mut self, rows: i64) {
        if self.document.is_empty() {
            return;
        }
        if matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            if let Err(err) = self.scroll_viewport_disassembly(rows) {
                self.set_error_status(err.to_string());
            }
            return;
        }
        let max_top = self.max_viewport_top();
        let delta = rows.saturating_mul(self.config.bytes_per_line as i64);
        self.viewport_top = if delta >= 0 {
            self.viewport_top.saturating_add(delta as u64).min(max_top)
        } else {
            self.viewport_top.saturating_sub(delta.unsigned_abs())
        };
        self.viewport_top =
            align_offset(self.viewport_top, self.config.bytes_per_line).min(max_top);
        self.clamp_cursor_into_view();
    }

    pub(crate) fn clamp_cursor_into_view(&mut self) {
        if matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            if let Err(err) = self.clamp_cursor_into_view_disassembly() {
                self.set_error_status(err.to_string());
            }
            return;
        }
        if self.document.is_empty() {
            self.cursor = 0;
            return;
        }
        let row_size = self.config.bytes_per_line as u64;
        let visible_rows = self.visible_rows();
        let visible_start = self.viewport_top;
        let visible_end = (self.viewport_top + visible_rows.saturating_mul(row_size))
            .min(self.document.len())
            .saturating_sub(1);
        let anchor = self
            .cursor_anchor_offset()
            .clamp(visible_start, visible_end);
        if self.mode_allows_eof_cursor()
            && self.cursor == self.document.len()
            && self.cursor_anchor_offset() == anchor
        {
            return;
        }
        self.cursor = anchor;
    }

    pub(crate) fn max_viewport_top(&self) -> u64 {
        if self.document.is_empty() {
            return 0;
        }
        let row_size = self.config.bytes_per_line as u64;
        let visible_rows = self.visible_rows();
        let tail_rows = self.document.len().saturating_sub(1) / row_size;
        tail_rows
            .saturating_sub(visible_rows.saturating_sub(1))
            .saturating_mul(row_size)
    }

    pub(crate) fn visible_rows(&self) -> u64 {
        self.view_rows.max(1) as u64
    }

    pub(crate) fn offset_with_delta(&self, current: u64, delta: i64) -> u64 {
        if self.document.is_empty() {
            return 0;
        }
        let max = self.cursor_max(self.mode_allows_eof_cursor());
        if delta >= 0 {
            current.saturating_add(delta as u64).min(max)
        } else {
            current.saturating_sub(delta.unsigned_abs()).min(max)
        }
    }

    pub(crate) fn clamp_offset(&self, offset: u64) -> u64 {
        self.clamp_offset_with_eof(offset, false)
    }

    pub(crate) fn clamp_cursor_for_mode(&self, offset: u64, mode: Mode) -> u64 {
        self.clamp_offset_with_eof(
            offset,
            matches!(mode, Mode::EditHex { .. } | Mode::InsertHex { .. }),
        )
    }

    pub(crate) fn mode_allows_eof_cursor(&self) -> bool {
        matches!(self.mode, Mode::EditHex { .. } | Mode::InsertHex { .. })
    }

    pub(crate) fn cursor_anchor_offset(&self) -> u64 {
        if self.document.is_empty() {
            0
        } else {
            self.cursor.min(self.document.len() - 1)
        }
    }

    fn cursor_max(&self, allow_eof: bool) -> u64 {
        if self.document.is_empty() {
            0
        } else if allow_eof {
            self.document.len()
        } else {
            self.document.len() - 1
        }
    }

    fn clamp_offset_with_eof(&self, offset: u64, allow_eof: bool) -> u64 {
        if self.document.is_empty() {
            0
        } else {
            offset.min(self.cursor_max(allow_eof))
        }
    }

    fn ensure_cursor_visible_disassembly(&mut self) -> crate::error::HxResult<()> {
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

    fn center_cursor_in_view_disassembly(&mut self) -> crate::error::HxResult<()> {
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

    fn scroll_viewport_disassembly(&mut self, rows: i64) -> crate::error::HxResult<()> {
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

    fn clamp_cursor_into_view_disassembly(&mut self) -> crate::error::HxResult<()> {
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

    fn current_disassembly_state(&self) -> Option<crate::disasm::DisassemblyState> {
        match &self.main_view {
            crate::app::MainView::Disassembly(state) => Some(state.clone()),
            crate::app::MainView::Hex => None,
        }
    }

    fn current_disassembly_viewport_top(&self) -> Option<u64> {
        self.current_disassembly_state()
            .map(|state| state.viewport_top)
    }

    fn set_disassembly_viewport_top(&mut self, top: u64) {
        if let crate::app::MainView::Disassembly(state) = &mut self.main_view {
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
        state: &crate::disasm::DisassemblyState,
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
        state: &crate::disasm::DisassemblyState,
        offset: u64,
    ) -> crate::error::HxResult<Option<u64>> {
        let Some(region) = self.disassembly_visible_region_containing(state, offset) else {
            return Ok(None);
        };
        if !region.executable {
            return Ok(Some(self.data_row_start(region.start, offset)));
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
        state: &crate::disasm::DisassemblyState,
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
        state: &crate::disasm::DisassemblyState,
        region: &crate::executable::CodeSpan,
        limit_exclusive: u64,
    ) -> crate::error::HxResult<Vec<u64>> {
        if !region.executable {
            return Ok(self.data_row_offsets_before(
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
        state: &crate::disasm::DisassemblyState,
        offset: u64,
    ) -> Option<crate::executable::CodeSpan> {
        self.disassembly_visible_regions(state)
            .into_iter()
            .find(|region| region.contains(offset))
    }

    fn disassembly_visible_regions(
        &self,
        state: &crate::disasm::DisassemblyState,
    ) -> Vec<crate::executable::CodeSpan> {
        if self.document.is_empty() {
            return Vec::new();
        }

        let mut spans = state.info.code_spans.clone();
        spans.sort_by_key(|span| (span.start, span.end_inclusive - span.start));

        let mut regions = Vec::new();
        let mut cursor = 0_u64;
        let file_end = self.document.len().saturating_sub(1);

        for span in spans {
            if span.start > file_end {
                continue;
            }
            if span.start > cursor {
                regions.push(crate::executable::CodeSpan {
                    start: cursor,
                    end_inclusive: span.start - 1,
                    name: Some("<raw>".to_owned()),
                    executable: false,
                });
            }
            let start = span.start.max(cursor);
            let end = span.end_inclusive.min(file_end);
            if start <= end {
                regions.push(crate::executable::CodeSpan {
                    start,
                    end_inclusive: end,
                    name: span.name.clone(),
                    executable: span.executable,
                });
                cursor = end.saturating_add(1);
            }
            if cursor > file_end {
                break;
            }
        }

        if cursor <= file_end {
            regions.push(crate::executable::CodeSpan {
                start: cursor,
                end_inclusive: file_end,
                name: Some("<raw>".to_owned()),
                executable: false,
            });
        }

        regions
    }

    fn data_row_start(&self, region_start: u64, offset: u64) -> u64 {
        let step = 8_u64;
        region_start + ((offset.saturating_sub(region_start)) / step) * step
    }

    fn data_row_offsets_before(
        &self,
        region_start: u64,
        region_end: u64,
        limit_exclusive: u64,
    ) -> Vec<u64> {
        let capped_limit = limit_exclusive.min(region_end.saturating_add(1));
        if capped_limit <= region_start {
            return Vec::new();
        }
        let mut offsets = Vec::new();
        let mut current = region_start;
        while current < capped_limit {
            offsets.push(current);
            current = current.saturating_add(8);
        }
        offsets
    }
}

pub(crate) fn align_offset(offset: u64, bytes_per_line: usize) -> u64 {
    if bytes_per_line == 0 {
        offset
    } else {
        offset / bytes_per_line as u64 * bytes_per_line as u64
    }
}
