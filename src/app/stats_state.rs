use std::time::{Duration, Instant};

use crate::app::{App, SidePanelKind, StatsScope, StatsState};
use crate::byte_stats::{ByteStats, BYTE_VALUE_COUNT};
use crate::commands::types::StatsCommand;
use crate::core::document::walk::WalkControl;
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

const STATS_LOGICAL_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const STATS_TOP_INITIAL_LIMIT: usize = 16;
const STATS_TOP_EXPAND_STEP: usize = 64;
#[cfg(test)]
pub(crate) const STATS_SYNC_LIMIT_BYTES: u64 = 256 * 1024;
#[cfg(not(test))]
pub(crate) const STATS_SYNC_LIMIT_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(test)]
pub(crate) const STATS_PROGRESS_STEP_BYTES: u64 = 128 * 1024;
#[cfg(not(test))]
pub(crate) const STATS_PROGRESS_STEP_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct StatsProgressScan {
    scope: StatsScope,
    start: u64,
    end: u64,
    cursor: u64,
    scanned_display: u64,
    stats: ByteStats,
    document_revision: u64,
    top_byte_limit: usize,
}

impl StatsProgressScan {
    fn new(
        scope: StatsScope,
        start: u64,
        end: u64,
        document_revision: u64,
        top_byte_limit: usize,
    ) -> Self {
        Self {
            scope,
            start,
            end,
            cursor: start,
            scanned_display: 0,
            stats: ByteStats::new(),
            document_revision,
            top_byte_limit,
        }
    }

    fn display_total(&self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }

    fn scope_label(&self) -> String {
        self.scope.label(self.start, self.end)
    }

    fn into_state(self) -> StatsState {
        StatsState {
            scope: self.scope,
            start: self.start,
            end: self.end,
            scanned_display: self.scanned_display,
            stats: self.stats,
            document_revision: self.document_revision,
            scroll_offset: 0,
            top_byte_limit: self.top_byte_limit,
        }
    }
}

impl App {
    pub(crate) fn stats_state(&self) -> Option<&StatsState> {
        self.stats_state.as_ref()
    }

    pub(crate) fn stats_state_mut(&mut self) -> Option<&mut StatsState> {
        self.stats_state.as_mut()
    }

    pub(super) fn execute_stats_command(&mut self, command: StatsCommand) -> HxResult<()> {
        match command {
            StatsCommand::Off => {
                self.close_stats_panel();
                Ok(())
            }
            StatsCommand::Refresh => self.refresh_stats_panel(),
            StatsCommand::Auto => {
                if let Some((start, end)) = self.active_selection_range() {
                    self.open_stats_range(StatsScope::Selection, start, end)
                } else {
                    self.open_stats_all()
                }
            }
            StatsCommand::All => self.open_stats_all(),
            StatsCommand::Selection => {
                let (start, end) = self
                    .active_selection_range()
                    .ok_or(HxError::MissingSelection)?;
                self.open_stats_range(StatsScope::Selection, start, end)
            }
        }
    }

    pub(crate) fn stats_scan_pending(&self) -> bool {
        self.pending_stats_scan.is_some()
    }

    pub(crate) fn cancel_stats_scan(&mut self, message: Option<&str>) {
        self.pending_stats_scan = None;
        if self.stats_state.is_none() && self.active_side_panel == SidePanelKind::Stats {
            self.restore_inspector_after_side_panel_close();
        }
        if let Some(message) = message {
            self.set_info_status(message);
        }
    }

    pub(crate) fn report_stats_scan_blocked_input(&mut self) {
        self.set_stats_scan_status();
    }

    pub(crate) fn continue_stats_scan(&mut self) -> HxResult<()> {
        let Some(mut scan) = self.pending_stats_scan.take() else {
            return Ok(());
        };

        if scan.cursor > scan.end {
            self.finish_stats_scan(scan);
            return Ok(());
        }

        let started = Instant::now();
        let mut step_display = 0_u64;

        while scan.cursor <= scan.end && step_display < STATS_PROGRESS_STEP_BYTES {
            let remaining_step = STATS_PROGRESS_STEP_BYTES.saturating_sub(step_display);
            let chunk_limit = (STATS_LOGICAL_CHUNK_BYTES as u64)
                .min(remaining_step)
                .max(1);
            let chunk_end = scan
                .cursor
                .saturating_add(chunk_limit)
                .saturating_sub(1)
                .min(scan.end);
            let display_scanned = chunk_end.saturating_sub(scan.cursor).saturating_add(1);

            self.document.walk_logical_chunks(
                scan.cursor,
                chunk_end,
                STATS_LOGICAL_CHUNK_BYTES,
                |chunk| {
                    scan.stats.update(chunk.bytes);
                    Ok(WalkControl::Continue)
                },
            )?;

            scan.scanned_display = scan.scanned_display.saturating_add(display_scanned);
            step_display = step_display.saturating_add(display_scanned);
            if chunk_end >= scan.end {
                self.finish_stats_scan(scan);
                return Ok(());
            }
            scan.cursor = chunk_end.saturating_add(1);
            if started.elapsed() >= stats_step_time_budget() {
                break;
            }
        }

        self.pending_stats_scan = Some(scan);
        self.set_stats_scan_status();
        Ok(())
    }

    pub(crate) fn scroll_stats_panel(&mut self, rows: i64) {
        let visible_rows = self.side_panel_visible_rows();
        let total_rows = self
            .stats_state()
            .map(|state| {
                crate::view::stats_panel::line_count(
                    state,
                    state.document_revision != self.document_revision,
                )
            })
            .unwrap_or(2);
        let max_scroll = total_rows.saturating_sub(visible_rows);
        let Some(state) = self.stats_state_mut() else {
            return;
        };
        state.scroll_offset = if rows >= 0 {
            state
                .scroll_offset
                .saturating_add(rows as usize)
                .min(max_scroll)
        } else {
            state.scroll_offset.saturating_sub((-rows) as usize)
        };
    }

    pub(crate) fn expand_stats_top_bytes(&mut self) {
        let Some((visible, unique)) = self.stats_state_mut().map(|state| {
            let unique = state.stats.unique_count();
            let current = state.clamped_top_byte_limit();
            if current < unique && current < BYTE_VALUE_COUNT {
                state.top_byte_limit = current
                    .saturating_add(STATS_TOP_EXPAND_STEP)
                    .min(BYTE_VALUE_COUNT);
            }
            (state.clamped_top_byte_limit().min(unique), unique)
        }) else {
            return;
        };

        if visible >= unique {
            self.set_info_status(format!(
                "stats: top bytes showing all {unique} observed byte values"
            ));
        } else {
            self.set_info_status(format!(
                "stats: top bytes showing {visible} / {unique} observed byte values"
            ));
        }
    }

    fn open_stats_all(&mut self) -> HxResult<()> {
        if self.document.is_empty() {
            self.stats_state = None;
            self.pending_stats_scan = None;
            self.set_info_status("stats: no data");
            return Ok(());
        }
        self.open_stats_range(StatsScope::EntireFile, 0, self.document.len() - 1)
    }

    fn refresh_stats_panel(&mut self) -> HxResult<()> {
        if self.pending_stats_scan.is_some() {
            self.set_stats_scan_status();
            return Ok(());
        }
        let Some(state) = self.stats_state().cloned() else {
            return self.execute_stats_command(StatsCommand::Auto);
        };
        if self.document.is_empty() {
            self.stats_state = None;
            self.set_info_status("stats: no data");
            return Ok(());
        }
        let last = self.document.len() - 1;
        let start = state.start.min(last);
        let end = state.end.min(last);
        if start > end {
            self.stats_state = None;
            self.set_info_status("stats: range is no longer valid");
            return Ok(());
        }
        self.open_stats_range(state.scope, start, end)
    }

    fn open_stats_range(&mut self, scope: StatsScope, start: u64, end: u64) -> HxResult<()> {
        let display_total = end.saturating_sub(start).saturating_add(1);
        let top_byte_limit = self
            .stats_state()
            .map(StatsState::clamped_top_byte_limit)
            .unwrap_or(STATS_TOP_INITIAL_LIMIT);
        self.open_empty_stats_panel();
        if display_total > STATS_SYNC_LIMIT_BYTES {
            self.start_stats_scan(scope, start, end, top_byte_limit);
            return Ok(());
        }

        let stats = self.compute_stats_range(start, end)?;
        let logical_bytes = stats.logical_bytes();
        self.stats_state = Some(StatsState {
            scope,
            start,
            end,
            scanned_display: display_total,
            stats,
            document_revision: self.document_revision,
            scroll_offset: 0,
            top_byte_limit,
        });
        self.set_stats_result_status(scope.label(start, end), logical_bytes);
        Ok(())
    }

    fn open_empty_stats_panel(&mut self) {
        self.clear_diff_cell_selection();
        self.show_side_panel = true;
        self.active_side_panel = SidePanelKind::Stats;
        self.mode = Mode::SidePanel;
        self.stats_state = None;
        self.pending_stats_scan = None;
    }

    fn close_stats_panel(&mut self) {
        let was_stats = self.active_side_panel == SidePanelKind::Stats;
        self.stats_state = None;
        self.pending_stats_scan = None;
        if was_stats {
            self.restore_inspector_after_side_panel_close();
            self.show_side_panel = false;
            if self.mode.is_side_panel() {
                self.mode = Mode::Normal;
            }
            self.set_info_status("stats off");
        }
    }

    fn start_stats_scan(&mut self, scope: StatsScope, start: u64, end: u64, top_byte_limit: usize) {
        self.pending_stats_scan = Some(StatsProgressScan::new(
            scope,
            start,
            end,
            self.document_revision,
            top_byte_limit,
        ));
        self.set_stats_scan_status();
    }

    fn compute_stats_range(&mut self, start: u64, end: u64) -> HxResult<ByteStats> {
        let mut stats = ByteStats::new();
        self.document
            .walk_logical_chunks(start, end, STATS_LOGICAL_CHUNK_BYTES, |chunk| {
                stats.update(chunk.bytes);
                Ok(WalkControl::Continue)
            })?;
        Ok(stats)
    }

    fn finish_stats_scan(&mut self, scan: StatsProgressScan) {
        let scope = scan.scope_label();
        let logical_bytes = scan.stats.logical_bytes();
        self.stats_state = Some(scan.into_state());
        self.active_side_panel = SidePanelKind::Stats;
        self.show_side_panel = true;
        if self.mode.is_side_panel() {
            self.mode = Mode::SidePanel;
        }
        self.set_stats_result_status(scope, logical_bytes);
    }

    fn set_stats_result_status(&mut self, scope: String, logical_bytes: u64) {
        if logical_bytes == 0 {
            self.set_info_status(format!("stats [{}]: no logical bytes", scope));
        } else if let Some(state) = self.stats_state() {
            self.set_info_status(format!(
                "stats [{}]: {} logical bytes, entropy {:.3} bits/byte",
                scope,
                format_stats_progress_bytes(logical_bytes),
                state.stats.entropy_bits_per_byte()
            ));
        }
    }

    fn set_stats_scan_status(&mut self) {
        let message = {
            let Some(scan) = self.pending_stats_scan.as_ref() else {
                return;
            };
            let total = scan.display_total();
            let percent = if total == 0 {
                100.0
            } else {
                (scan.scanned_display as f64 / total as f64 * 100.0).min(100.0)
            };
            format!(
                "stats [{}]... {} / {} checked ({percent:.0}%); {} logical counted; Esc to cancel",
                scan.scope_label(),
                format_stats_progress_bytes(scan.scanned_display),
                format_stats_progress_bytes(total),
                format_stats_progress_bytes(scan.stats.logical_bytes())
            )
        };
        self.set_info_status(message);
    }
}

#[cfg(test)]
fn stats_step_time_budget() -> Duration {
    Duration::from_millis(0)
}

#[cfg(not(test))]
fn stats_step_time_budget() -> Duration {
    Duration::from_millis(8)
}

fn format_stats_progress_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} bytes")
    }
}
