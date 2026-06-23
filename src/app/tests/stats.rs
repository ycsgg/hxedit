use super::*;
use crate::commands::types::StatsCommand;

#[test]
fn stats_command_counts_active_selection() {
    let mut app = app_with_bytes(&[0x00, 0x00, 0xff, b'A', b'A', b'B']);
    app.toggle_visual();
    app.move_horizontal(3);

    app.execute_command(Command::Stats(StatsCommand::Auto))
        .unwrap();

    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Stats);
    assert!(!app.stats_scan_pending());
    let state = app.stats_state().unwrap();
    assert_eq!(state.scope, crate::app::StatsScope::Selection);
    assert_eq!((state.start, state.end), (0, 3));
    assert_eq!(state.stats.logical_bytes(), 4);
    assert_eq!(state.stats.count(0x00), 2);
    assert_eq!(state.stats.count(0xff), 1);
    assert_eq!(state.stats.count(b'A'), 1);
    assert!((state.stats.entropy_bits_per_byte() - 1.5).abs() < f64::EPSILON);
    assert!(app.status_message.contains("entropy 1.500"));
}

#[test]
fn stats_panel_expands_top_bytes_in_batches() {
    let bytes = (0_u8..=255).collect::<Vec<_>>();
    let mut app = app_with_bytes(&bytes);

    app.execute_command(Command::Stats(StatsCommand::All))
        .unwrap();

    assert_eq!(
        app.stats_state().unwrap().top_byte_limit,
        crate::app::stats_state::STATS_TOP_INITIAL_LIMIT
    );

    app.handle_action(Action::SidePanelEnter);
    assert_eq!(app.stats_state().unwrap().top_byte_limit, 80);
    assert!(app.status_message.contains("80 / 256"));

    app.handle_action(Action::SidePanelToggleCollapse);
    assert_eq!(app.stats_state().unwrap().top_byte_limit, 144);

    for _ in 0..4 {
        app.handle_action(Action::SidePanelEnter);
    }

    assert_eq!(
        app.stats_state().unwrap().top_byte_limit,
        crate::byte_stats::BYTE_VALUE_COUNT
    );
    assert!(app.status_message.contains("all 256 observed"));
}

#[test]
fn stats_command_scans_large_range_across_ticks() {
    let size = crate::app::stats_state::STATS_SYNC_LIMIT_BYTES + 1;
    let mut app = app_with_len(size as usize);

    app.execute_command(Command::Stats(StatsCommand::All))
        .unwrap();

    assert!(app.stats_scan_pending());
    assert_eq!(app.active_side_panel, crate::app::SidePanelKind::Stats);
    assert!(app.stats_state().is_none());
    assert!(app.status_message.contains("Esc to cancel"));

    app.continue_stats_scan().unwrap();
    assert!(app.stats_scan_pending());
    assert!(app.status_message.contains("logical counted"));

    let mut steps = 1;
    while app.stats_scan_pending() {
        app.continue_stats_scan().unwrap();
        steps += 1;
        assert!(steps <= 8);
    }

    assert!(steps > 1);
    let state = app.stats_state().unwrap();
    assert_eq!(state.scope, crate::app::StatsScope::EntireFile);
    assert_eq!(state.stats.logical_bytes(), size);
    assert_eq!(state.stats.count(0x00), size);
    assert_eq!(state.stats.unique_count(), 1);
    assert_eq!(state.stats.entropy_bits_per_byte(), 0.0);
}

#[test]
fn stats_scan_blocks_input_until_escape() {
    let size = crate::app::stats_state::STATS_SYNC_LIMIT_BYTES + 1;
    let mut app = app_with_len(size as usize);

    app.execute_command(Command::Stats(StatsCommand::All))
        .unwrap();
    assert!(app.stats_scan_pending());

    app.handle_action(Action::MoveDown);
    assert_eq!(app.cursor, 0);
    assert!(app.stats_scan_pending());
    assert!(app.status_message.contains("stats"));

    app.handle_action(Action::LeaveMode);
    assert!(!app.stats_scan_pending());
    assert!(app.status_message.contains("stats canceled"));
}
