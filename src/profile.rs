use std::time::Duration;

use crate::core::page_cache::CacheStats;

#[derive(Debug, Clone, Copy, Default)]
pub struct StartupStats {
    pub config_parse: Duration,
    pub document_open: Duration,
    pub app_init: Duration,
    pub terminal_setup: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderMainStats {
    pub rows: usize,
    pub row_collect: Duration,
    pub line_build: Duration,
    pub widget_draw: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStats {
    pub total: Duration,
    pub main: Duration,
    pub status: Duration,
    pub command: Duration,
    pub main_stats: RenderMainStats,
}

pub struct Profiler {
    startup: StartupStats,
    frame_count: u64,
    total_frame_time: Duration,
    slowest_frame: FrameStats,
    last_cache_stats: CacheStats,
    slow_frame_threshold: Duration,
    events: Vec<String>,
}

impl Profiler {
    pub fn new(startup: StartupStats) -> Self {
        Self {
            startup,
            frame_count: 0,
            total_frame_time: Duration::default(),
            slowest_frame: FrameStats::default(),
            last_cache_stats: CacheStats::default(),
            slow_frame_threshold: Duration::from_millis(16),
            events: Vec::new(),
        }
    }

    pub fn set_terminal_setup(&mut self, duration: Duration) {
        self.startup.terminal_setup = duration;
    }

    pub fn log_startup(&mut self, cache_stats: CacheStats) {
        self.events.push(format!(
            "[profile] startup config={:.3}ms open={:.3}ms app={:.3}ms terminal={:.3}ms io={}",
            ms(self.startup.config_parse),
            ms(self.startup.document_open),
            ms(self.startup.app_init),
            ms(self.startup.terminal_setup),
            fmt_cache(cache_stats),
        ));
        self.last_cache_stats = cache_stats;
    }

    pub fn record_frame(&mut self, frame: FrameStats, total_cache: CacheStats) {
        let delta = total_cache.delta_from(self.last_cache_stats);
        self.last_cache_stats = total_cache;
        self.frame_count += 1;
        self.total_frame_time += frame.total;
        if frame.total >= self.slowest_frame.total {
            self.slowest_frame = frame;
        }

        if self.frame_count == 1 {
            self.events.push(format!(
                "[profile] first-frame total={:.3}ms main={:.3}ms status={:.3}ms command={:.3}ms rows={} collect={:.3}ms build={:.3}ms draw={:.3}ms io={}",
                ms(frame.total),
                ms(frame.main),
                ms(frame.status),
                ms(frame.command),
                frame.main_stats.rows,
                ms(frame.main_stats.row_collect),
                ms(frame.main_stats.line_build),
                ms(frame.main_stats.widget_draw),
                fmt_cache(delta),
            ));
        } else if frame.total >= self.slow_frame_threshold {
            self.events.push(format!(
                "[profile] slow-frame #{} total={:.3}ms main={:.3}ms status={:.3}ms command={:.3}ms rows={} collect={:.3}ms build={:.3}ms draw={:.3}ms io={}",
                self.frame_count,
                ms(frame.total),
                ms(frame.main),
                ms(frame.status),
                ms(frame.command),
                frame.main_stats.rows,
                ms(frame.main_stats.row_collect),
                ms(frame.main_stats.line_build),
                ms(frame.main_stats.widget_draw),
                fmt_cache(delta),
            ));
        }
    }

    pub fn record_search(
        &mut self,
        kind: &str,
        direction: &str,
        pattern_len: usize,
        duration: Duration,
        found: Option<u64>,
        total_cache: CacheStats,
    ) {
        let delta = total_cache.delta_from(self.last_cache_stats);
        self.last_cache_stats = total_cache;
        let found = found
            .map(|offset| format!("0x{offset:x}"))
            .unwrap_or_else(|| "none".to_owned());
        self.events.push(format!(
            "[profile] search kind={} dir={} pattern={} time={:.3}ms found={} io={}",
            kind,
            direction,
            pattern_len,
            ms(duration),
            found,
            fmt_cache(delta),
        ));
    }

    pub fn print_report(&self, total_cache: CacheStats) {
        for event in &self.events {
            eprintln!("{event}");
        }
        if self.frame_count == 0 {
            return;
        }
        eprintln!(
            "[profile] summary frames={} avg={:.3}ms slowest={:.3}ms total-io={}",
            self.frame_count,
            ms(self.total_frame_time / self.frame_count as u32),
            ms(self.slowest_frame.total),
            fmt_cache(total_cache),
        );
    }
}

fn fmt_cache(stats: CacheStats) -> String {
    format!(
        "calls={} hits={} misses={} bytes={}",
        stats.read_range_calls, stats.page_hits, stats.page_misses, stats.bytes_returned
    )
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
