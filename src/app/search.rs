use std::time::Instant;

use crate::app::{App, SearchDirection, SearchState};
#[cfg(feature = "disasm")]
use crate::disasm::DisasmRowKind;
#[cfg(feature = "disasm")]
use crate::error::HxError;
use crate::error::HxResult;

impl App {
    pub(crate) fn repeat_search(&mut self, direction: SearchDirection) -> HxResult<()> {
        let Some(search) = self.last_search.clone() else {
            self.set_info_status("no active search");
            return Ok(());
        };
        self.run_search(&search, direction)
    }

    pub(crate) fn run_search(
        &mut self,
        search: &SearchState,
        direction: SearchDirection,
    ) -> HxResult<()> {
        let started_at = Instant::now();
        #[cfg(feature = "disasm")]
        let (found, wrapped) = if let Some(pattern) = search.byte_pattern() {
            self.run_byte_search(pattern, direction)?
        } else if let Some(pattern) = search.instruction_query() {
            self.run_instruction_search(pattern, direction)?
        } else {
            (None, false)
        };

        #[cfg(not(feature = "disasm"))]
        let (found, wrapped) = if let Some(pattern) = search.byte_pattern() {
            self.run_byte_search(pattern, direction)?
        } else {
            (None, false)
        };

        if let Some(profiler) = self.profiler.as_mut() {
            profiler.record_search(
                search.kind.label(),
                direction.label(),
                search.pattern_len(),
                started_at.elapsed(),
                found,
                self.document.io_stats(),
            );
        }

        if let Some(found) = found {
            self.cursor = found;
            if matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
                self.focus_disassembly_row_at_offset(found)?;
            }
            if wrapped {
                self.set_notice_status(format!(
                    "wrapped search: found {} at display 0x{:x}",
                    search.kind.label(),
                    found
                ));
            } else {
                self.set_info_status(format!(
                    "found {} at display 0x{:x}",
                    search.kind.label(),
                    found
                ));
            }
        } else {
            self.set_info_status(format!("{} pattern not found", search.kind.label()));
        }

        Ok(())
    }

    fn run_byte_search(
        &mut self,
        pattern: &[u8],
        direction: SearchDirection,
    ) -> HxResult<(Option<u64>, bool)> {
        Ok(match direction {
            SearchDirection::Forward => {
                let start = if self.document.is_empty() {
                    0
                } else {
                    (self.cursor + 1).min(self.document.len())
                };
                if let Some(found) = self.document.search_forward(start, pattern)? {
                    (Some(found), false)
                } else {
                    (self.document.search_forward(0, pattern)?, start > 0)
                }
            }
            SearchDirection::Backward => {
                if let Some(found) = self.document.search_backward(self.cursor, pattern)? {
                    (Some(found), false)
                } else {
                    (
                        self.document
                            .search_backward(self.document.len(), pattern)?,
                        self.cursor < self.document.len(),
                    )
                }
            }
        })
    }

    #[cfg(feature = "disasm")]
    fn run_instruction_search(
        &mut self,
        pattern: &str,
        direction: SearchDirection,
    ) -> HxResult<(Option<u64>, bool)> {
        let state = match &self.main_view {
            crate::app::MainView::Disassembly(state) => state.clone(),
            crate::app::MainView::Hex => {
                return Err(HxError::DisassemblyUnavailable(
                    "instruction search requires disassembly view; run :dis first".to_owned(),
                ));
            }
        };

        Ok(match direction {
            SearchDirection::Forward => {
                let start = self.cursor_anchor_offset().saturating_add(1);
                if let Some(found) = self.search_instruction_forward_from(&state, pattern, start)? {
                    (Some(found), false)
                } else {
                    (
                        self.search_instruction_forward_from(&state, pattern, 0)?,
                        start > 0,
                    )
                }
            }
            SearchDirection::Backward => {
                let limit_exclusive = self.cursor_anchor_offset();
                if let Some(found) =
                    self.search_instruction_backward_before(&state, pattern, limit_exclusive)?
                {
                    (Some(found), false)
                } else {
                    (
                        self.search_instruction_backward_before(
                            &state,
                            pattern,
                            self.document.len(),
                        )?,
                        limit_exclusive < self.document.len(),
                    )
                }
            }
        })
    }

    #[cfg(feature = "disasm")]
    fn search_instruction_forward_from(
        &mut self,
        state: &crate::disasm::DisassemblyState,
        pattern: &str,
        start: u64,
    ) -> HxResult<Option<u64>> {
        if self.document.is_empty() {
            return Ok(None);
        }

        let mut cursor = start.min(self.document.len().saturating_sub(1));
        while cursor < self.document.len() {
            let rows = self.collect_disassembly_rows(state, cursor, 256)?;
            if rows.is_empty() {
                break;
            }
            let mut next_cursor = cursor;
            for row in rows {
                let row_end = row.offset + row.len() as u64 - 1;
                next_cursor = row_end.saturating_add(1);
                if start > row.offset && start <= row_end {
                    continue;
                }
                if row.kind == DisasmRowKind::Instruction
                    && row.text.to_ascii_lowercase().contains(pattern)
                {
                    return Ok(Some(row.offset));
                }
            }
            if next_cursor <= cursor {
                break;
            }
            cursor = next_cursor;
        }
        Ok(None)
    }

    #[cfg(feature = "disasm")]
    fn search_instruction_backward_before(
        &mut self,
        state: &crate::disasm::DisassemblyState,
        pattern: &str,
        limit_exclusive: u64,
    ) -> HxResult<Option<u64>> {
        if self.document.is_empty() || limit_exclusive == 0 {
            return Ok(None);
        }

        let mut cursor = 0_u64;
        let mut last_match = None;
        while cursor < self.document.len() {
            let rows = self.collect_disassembly_rows(state, cursor, 256)?;
            if rows.is_empty() {
                break;
            }
            let mut next_cursor = cursor;
            for row in rows {
                let row_end = row.offset + row.len() as u64 - 1;
                if row_end >= limit_exclusive {
                    return Ok(last_match);
                }
                next_cursor = row_end.saturating_add(1);
                if row.kind == DisasmRowKind::Instruction
                    && row.text.to_ascii_lowercase().contains(pattern)
                {
                    last_match = Some(row.offset);
                }
            }
            if next_cursor <= cursor {
                break;
            }
            cursor = next_cursor;
        }
        Ok(last_match)
    }
}
