use ratatui::text::{Line, Span};

use crate::app::StatsState;
use crate::view::palette::Palette;

pub(crate) fn line_count(state: &StatsState, stale: bool) -> usize {
    let top_count = state.stats.top_bytes(state.clamped_top_byte_limit()).len();
    28 + usize::from(stale) + top_count
}

pub(crate) fn build_lines(
    state: &StatsState,
    width: u16,
    stale: bool,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut lines = Vec::with_capacity(line_count(state, stale));
    lines.push(Line::styled("Byte Stats", palette.inspector_header));
    if stale {
        lines.push(Line::styled("stale: run :stats refresh", palette.warning));
    }

    let total = state.stats.logical_bytes();
    lines.push(metric_line("scope", &state.scope_label(), palette));
    lines.push(metric_line(
        "display",
        &format!(
            "0x{:x}-0x{:x} ({} checked)",
            state.start,
            state.end,
            format_count(state.scanned_display)
        ),
        palette,
    ));
    lines.push(metric_line("logical", &format_count(total), palette));
    lines.push(metric_line(
        "entropy",
        &format!(
            "{:.4} bits/byte ({:.1}%)",
            state.stats.entropy_bits_per_byte(),
            entropy_percent(state.stats.entropy_bits_per_byte())
        ),
        palette,
    ));
    lines.push(metric_line(
        "unique",
        &format!("{} / 256", state.stats.unique_count()),
        palette,
    ));
    lines.push(metric_line(
        "classes",
        &format!(
            "NUL {}  FF {}",
            format_count(state.stats.count(0x00)),
            format_count(state.stats.count(0xff))
        ),
        palette,
    ));
    lines.push(metric_line(
        "ascii",
        &format!(
            "printable {}  whitespace {}  control {}",
            format_count(state.stats.ascii_printable_count()),
            format_count(state.stats.ascii_whitespace_count()),
            format_count(state.stats.ascii_control_count())
        ),
        palette,
    ));

    lines.push(Line::raw(""));
    lines.push(top_bytes_header_line(state, palette));
    let bar_width = bar_width(width);
    let top = state.stats.top_bytes(state.clamped_top_byte_limit());
    let max = top.first().map(|entry| entry.count).unwrap_or(0);
    for entry in top {
        lines.push(distribution_line(
            &byte_label(entry.byte),
            entry.count,
            total,
            max,
            bar_width,
            palette,
        ));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled("All byte ranges", palette.inspector_header));
    let buckets = state.stats.high_nibble_buckets();
    let max_bucket = buckets.iter().copied().max().unwrap_or(0);
    for (nibble, count) in buckets.into_iter().enumerate() {
        lines.push(byte_range_distribution_line(
            nibble as u8,
            count,
            total,
            max_bucket,
            range_bar_width(width),
            state,
            palette,
        ));
    }

    lines
}

fn top_bytes_header_line(state: &StatsState, palette: &Palette) -> Line<'static> {
    let unique = state.stats.unique_count();
    let visible = state.clamped_top_byte_limit().min(unique);
    Line::styled(
        format!("Top bytes {visible}/{unique}"),
        palette.inspector_header,
    )
}

fn metric_line(label: &str, value: &str, palette: &Palette) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<9}"), palette.inspector_field),
        Span::styled(value.to_owned(), palette.inspector_value),
    ])
}

fn distribution_line(
    label: &str,
    count: u64,
    total: u64,
    max: u64,
    bar_width: usize,
    palette: &Palette,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), palette.inspector_field),
        Span::styled(
            format!(
                "{:>10} {:>6.2}% ",
                format_count(count),
                percent(count, total)
            ),
            palette.inspector_value,
        ),
        Span::styled(bar(count, max, bar_width), palette.inspector_active),
    ])
}

fn byte_range_distribution_line(
    high_nibble: u8,
    count: u64,
    total: u64,
    max: u64,
    bar_width: usize,
    state: &StatsState,
    palette: &Palette,
) -> Line<'static> {
    let start = high_nibble << 4;
    let end = start | 0x0f;
    let peak = peak_byte_in_range(start, end, state)
        .map(|byte| format!(" max {}", byte_label(byte)))
        .unwrap_or_else(|| " max none".to_owned());
    Line::from(vec![
        Span::styled(format!("0x{start:02x}-{end:02x} "), palette.inspector_field),
        Span::styled(
            format!(
                "{:>10} {:>6.2}% ",
                format_count(count),
                percent(count, total)
            ),
            palette.inspector_value,
        ),
        Span::styled(bar(count, max, bar_width), palette.inspector_active),
        Span::styled(peak, palette.inspector_value),
    ])
}

fn peak_byte_in_range(start: u8, end: u8, state: &StatsState) -> Option<u8> {
    (start..=end)
        .filter(|byte| state.stats.count(*byte) != 0)
        .max_by_key(|byte| (state.stats.count(*byte), std::cmp::Reverse(*byte)))
}

fn byte_label(byte: u8) -> String {
    match byte {
        0x00 => "0x00 NUL".to_owned(),
        b'\t' => "0x09 TAB".to_owned(),
        b'\n' => "0x0a LF".to_owned(),
        b'\r' => "0x0d CR".to_owned(),
        b' ' => "0x20 SPACE".to_owned(),
        0x7f => "0x7f DEL".to_owned(),
        0x21..=0x7e => format!("0x{byte:02x} {}", byte as char),
        _ => format!("0x{byte:02x}"),
    }
}

fn bar(count: u64, max: u64, width: usize) -> String {
    if count == 0 || max == 0 || width == 0 {
        return String::new();
    }
    let filled = ((count as f64 / max as f64) * width as f64).ceil() as usize;
    "#".repeat(filled.min(width))
}

fn bar_width(width: usize) -> usize {
    width.saturating_sub(34).clamp(4, 28)
}

fn range_bar_width(width: usize) -> usize {
    width.saturating_sub(48).clamp(4, 20)
}

fn entropy_percent(bits_per_byte: f64) -> f64 {
    (bits_per_byte / 8.0 * 100.0).clamp(0.0, 100.0)
}

fn percent(count: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64 * 100.0
    }
}

fn format_count(count: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if count >= GIB {
        format!("{:.1} GiB", count as f64 / GIB as f64)
    } else if count >= MIB {
        format!("{:.1} MiB", count as f64 / MIB as f64)
    } else if count >= KIB {
        format!("{:.1} KiB", count as f64 / KIB as f64)
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::StatsScope;
    use crate::byte_stats::ByteStats;
    use crate::view::palette::{ColorLevel, Palette};

    #[test]
    fn renders_core_metrics() {
        let mut stats = ByteStats::new();
        stats.update(&[0, 0, 0xff, b'A']);
        let state = StatsState {
            scope: StatsScope::EntireFile,
            start: 0,
            end: 3,
            scanned_display: 4,
            stats,
            document_revision: 0,
            scroll_offset: 0,
            top_byte_limit: 16,
        };
        let lines = build_lines(&state, 64, false, &Palette::new(ColorLevel::NoColor));
        assert_eq!(line_count(&state, false), lines.len());
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Byte Stats"));
        assert!(rendered.contains("entropy"));
        assert!(rendered.contains("0x00 NUL"));
        assert!(rendered.contains("Top bytes 3/3"));
        assert!(rendered.contains("All byte ranges"));
        assert!(rendered.contains("max 0x00 NUL"));
    }
}
