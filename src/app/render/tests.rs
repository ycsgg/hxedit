use std::fs;

use tempfile::tempdir;

use super::App;
use crate::cli::Cli;
use crate::commands::types::Command;

pub(super) fn app_with_bytes(bytes: &[u8]) -> App {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    fs::write(&file, bytes).unwrap();
    let cli = Cli {
        file,
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
    };
    let mut app = App::from_cli(cli).unwrap();
    app.view_rows = 2;
    app
}

#[test]
fn visible_search_matches_collects_all_hits_on_screen() {
    let mut app = app_with_bytes(b"aba xx aba");
    app.execute_command(Command::SearchAscii {
        pattern: b"aba".to_vec(),
        backward: false,
    })
    .unwrap();

    let visible_rows = app.collect_visible_rows(1);
    assert_eq!(
        app.visible_search_matches(&visible_rows),
        vec![(0, 2), (7, 9)]
    );
}

#[test]
fn visible_diff_page_marks_equal_replace_and_missing_sides() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    fs::write(&other, b"axcde").unwrap();
    let mut app = app_with_bytes(b"abcd");
    app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();

    let visible_rows = app.collect_visible_rows(1);
    let page = app.visible_diff_page(&visible_rows).unwrap();
    let row = &page.rows[0];
    assert_eq!(
        row.cells[0].kind,
        crate::view::diff_panel::DiffPanelCellKind::Equal
    );
    assert_eq!(
        row.cells[1].kind,
        crate::view::diff_panel::DiffPanelCellKind::Replace
    );
    assert_eq!(
        row.cells[4].kind,
        crate::view::diff_panel::DiffPanelCellKind::OnlyOther
    );
    assert_eq!(
        page.main_rows[0][4].diff,
        Some(crate::view::hex_grid::DiffOverlayKind::OnlyOther)
    );

    let dir = tempdir().unwrap();
    let other = dir.path().join("other-short.bin");
    fs::write(&other, b"abc").unwrap();
    let mut app = app_with_bytes(b"abcd");
    app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();
    let visible_rows = app.collect_visible_rows(1);
    let page = app.visible_diff_page(&visible_rows).unwrap();
    assert_eq!(
        page.rows[0].cells[3].kind,
        crate::view::diff_panel::DiffPanelCellKind::OnlyCurrent
    );
}

#[test]
fn visible_diff_page_realizes_after_current_side_insert() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    fs::write(&other, b"abcdefghijklmnopqrstuvwxyz0123456789").unwrap();
    let mut app = app_with_bytes(b"abXcdefghijklmnopqrstuvwxyz0123456789");
    app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();

    let visible_rows = app.collect_visible_rows(1);
    let page = app.visible_diff_page(&visible_rows).unwrap();
    let row = &page.rows[0];
    assert_eq!(
        row.cells[0].kind,
        crate::view::diff_panel::DiffPanelCellKind::Equal
    );
    assert_eq!(
        row.cells[1].kind,
        crate::view::diff_panel::DiffPanelCellKind::Equal
    );
    assert_eq!(
        row.cells[2].kind,
        crate::view::diff_panel::DiffPanelCellKind::OnlyCurrent
    );
    assert_eq!(row.cells[2].other_byte, None);
    assert_eq!(
        row.cells[3].kind,
        crate::view::diff_panel::DiffPanelCellKind::Equal
    );
    assert_eq!(row.cells[3].other_byte, Some(b'c'));
    assert_eq!(
        row.cells[4].kind,
        crate::view::diff_panel::DiffPanelCellKind::Equal
    );
    assert_eq!(row.cells[4].other_byte, Some(b'd'));
}

#[test]
fn visible_diff_page_realizes_zip_like_mid_file_insert() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    let base = (0..0x140)
        .map(|idx| (idx as u8).wrapping_mul(37).wrapping_add(11))
        .collect::<Vec<_>>();
    fs::write(&other, &base).unwrap();
    let mut current = base.clone();
    current.insert(0xba, 0xab);
    let mut app = app_with_bytes(&current);
    app.viewport_top = 0xb0;
    app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();

    let visible_rows = app.collect_visible_rows(1);
    let page = app.visible_diff_page(&visible_rows).unwrap();
    let row = &page.rows[0];
    assert_eq!(
        row.cells[0x0a].kind,
        crate::view::diff_panel::DiffPanelCellKind::OnlyCurrent
    );
    assert_eq!(row.cells[0x0a].other_byte, None);
    assert_eq!(
        row.cells[0x0b].kind,
        crate::view::diff_panel::DiffPanelCellKind::Equal
    );
    assert_eq!(row.cells[0x0b].other_byte, Some(base[0xba]));
}

#[test]
fn visible_diff_page_shows_other_side_insert_as_placeholder_on_left() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    let base = (0..0x140)
        .map(|idx| (idx as u8).wrapping_mul(37).wrapping_add(11))
        .collect::<Vec<_>>();
    let mut other_bytes = base.clone();
    other_bytes.insert(0xba, 0xab);
    fs::write(&other, &other_bytes).unwrap();
    let mut app = app_with_bytes(&base);
    app.viewport_top = 0xb0;
    app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();

    let visible_rows = app.collect_visible_rows(1);
    let page = app.visible_diff_page(&visible_rows).unwrap();
    let row = &page.rows[0];
    assert_eq!(
        row.cells[0x0a].kind,
        crate::view::diff_panel::DiffPanelCellKind::OnlyOther
    );
    assert_eq!(row.cells[0x0a].other_byte, Some(0xab));
    assert_eq!(
        page.main_rows[0][0x0a].slot,
        crate::core::document::ByteSlot::Empty
    );
    assert_eq!(
        page.main_rows[0][0x0a].diff,
        Some(crate::view::hex_grid::DiffOverlayKind::OnlyOther)
    );
    assert_eq!(page.main_rows[0][0x0a].display_offset, None);
    assert_eq!(page.main_rows[0][0x0a].visual_offset, Some(0xba));
    assert_eq!(row.cells[0x0a].current_display_offset, None);
    assert_eq!(row.cells[0x0a].visual_display_offset, Some(0xba));
    assert_eq!(row.cells[0x0a].other_offset, Some(0xba));
    assert_eq!(
        row.cells[0x0b].kind,
        crate::view::diff_panel::DiffPanelCellKind::Equal
    );
    assert_eq!(row.cells[0x0b].other_byte, Some(base[0xba]));
    assert_eq!(page.main_rows[0][0x0b].display_offset, Some(0xba));
}

#[test]
fn diff_overlay_is_removed_when_diff_side_panel_is_not_active() {
    let dir = tempdir().unwrap();
    let other = dir.path().join("other.bin");
    fs::write(&other, b"abXc").unwrap();
    let mut app = app_with_bytes(b"abc");
    app.execute_command(Command::Diff(crate::commands::types::DiffCommand::Open {
        path: other,
        max_shift: None,
    }))
    .unwrap();
    let visible_rows = app.collect_visible_rows(1);
    assert!(!app
        .visible_diff_page(&visible_rows)
        .unwrap()
        .main_rows
        .is_empty());

    app.show_side_panel = false;
    app.active_side_panel = crate::app::SidePanelKind::Inspector;
    let hidden_page = app.visible_diff_page(&visible_rows).unwrap();
    assert!(hidden_page.rows.is_empty());
    assert!(hidden_page.main_rows.is_empty());
    assert!(hidden_page.overlay_spans.is_empty());
}

#[cfg(feature = "disasm-capstone")]
#[test]
fn disassembly_main_view_renders_decoded_instruction_lines() {
    let mut app = app_with_bytes(&{
        let mut bytes = vec![0_u8; 0x200];
        bytes[0..4].copy_from_slice(b"ELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        let ph = 64usize;
        bytes[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[ph + 4..ph + 8].copy_from_slice(&0x5u32.to_le_bytes());
        bytes[ph + 8..ph + 16].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&6u64.to_le_bytes());
        bytes[0x100..0x106].copy_from_slice(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]);
        bytes
    });
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    let lines = app.build_disassembly_lines(4);
    match lines.pane {
        super::MainPaneLines::Disassembly { text, .. } => {
            let joined = text
                .iter()
                .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
                .collect::<String>();
            assert!(joined.contains("push"));
            assert!(joined.contains("mov"));
            assert!(joined.contains("ret"));
        }
        _ => panic!("expected disassembly pane"),
    }
}
