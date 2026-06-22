use super::*;

fn gif_inspector_app() -> App {
    let mut app = app_with_bytes(b"GIF89a\x01\x00\x01\x00\x00\x00\x00;");
    app.execute_command(Command::Format {
        name: Some("gif".to_owned()),
    })
    .unwrap();
    let version_row = app
        .inspector()
        .unwrap()
        .rows
        .iter()
        .position(|row| {
            matches!(
                row,
                crate::format::parse::InspectorRow::Field { name, .. } if name == "version"
            )
        })
        .unwrap();
    app.inspector_state.as_mut().unwrap().selected_row = version_row;
    app.sync_cursor_to_inspector();
    assert_eq!(app.mode, Mode::SidePanel);
    assert_eq!(app.cursor, 3);
    app
}

fn type_command(app: &mut App, input: &str) {
    app.handle_action(Action::EnterCommand);
    for ch in input.chars() {
        app.handle_action(Action::CommandChar(ch));
    }
    app.handle_action(Action::CommandSubmit);
}

fn type_inspector_value(app: &mut App, input: &str) {
    app.handle_action(Action::SidePanelEnter);
    assert_eq!(app.mode, Mode::InspectorEdit);
    app.handle_action(Action::SidePanelHome);
    for _ in 0..8 {
        app.handle_action(Action::SidePanelDelete);
    }
    for ch in input.chars() {
        app.handle_action(Action::SidePanelChar(ch));
    }
    app.handle_action(Action::SidePanelEnter);
}

#[test]
fn paste_and_visual_delete_keep_undo_redo_semantics() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13, 0x14]);
    app.cursor = 1;

    assert_eq!(app.apply_paste_overwrite(&[0xaa, 0xbb]).unwrap(), 2);
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0xaa, 0xbb, 0x13, 0x14]
    );
    app.undo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0x11, 0x12, 0x13, 0x14]
    );
    app.redo(1, true).unwrap();
    assert_eq!(
        app.document.logical_bytes(0, 4).unwrap(),
        vec![0x10, 0xaa, 0xbb, 0x13, 0x14]
    );

    app.cursor = 3;
    assert_eq!(app.apply_paste_insert(&[0xcc]).unwrap(), 1);
    assert_eq!(
        app.document.logical_bytes(0, 5).unwrap(),
        vec![0x10, 0xaa, 0xbb, 0xcc, 0x13, 0x14]
    );
    app.undo(1, true).unwrap();
    assert_eq!(app.document.len(), 5);
    app.redo(1, true).unwrap();
    assert_eq!(app.document.len(), 6);
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0xcc));

    app.cursor = 1;
    app.toggle_visual();
    app.move_horizontal(2);
    app.delete_at_cursor_or_selection().unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.cursor, 1);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Deleted);
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Deleted);

    app.undo(1, true).unwrap();
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
    assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
    assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0xcc));

    app.redo(1, true).unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Deleted);
}

#[test]
fn inspector_edit_submits_and_roundtrips_through_undo_redo() {
    let mut app = gif_inspector_app();

    type_inspector_value(&mut app, "87a");

    assert_eq!(app.mode, Mode::SidePanel);
    assert_eq!(app.cursor, 3);
    assert_eq!(app.document.logical_bytes(3, 5).unwrap(), b"87a");
    assert!(app.status_message.contains("edited field at 0x3"));

    app.undo(1, true).unwrap();
    assert_eq!(app.mode, Mode::InspectorEdit);
    assert_eq!(app.document.logical_bytes(3, 5).unwrap(), b"89a");

    app.redo(1, true).unwrap();
    assert_eq!(app.mode, Mode::SidePanel);
    assert_eq!(app.document.logical_bytes(3, 5).unwrap(), b"87a");
}

#[test]
fn command_undo_clamps_cursor_after_length_changing_edit() {
    let mut app = app_with_bytes(&[0x11]);
    app.mode = Mode::InsertHex { pending: None };
    app.cursor = app.document.len();
    app.apply_paste_insert(&[0xaa, 0xbb]).unwrap();

    assert_eq!(app.document.len(), 3);
    assert_eq!(app.cursor, 2);

    app.execute_command(Command::Undo { steps: 1 }).unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.document.len(), 1);
    assert_eq!(app.cursor, 0);

    app.execute_command(Command::Redo { steps: 1 }).unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.document.len(), 3);
    assert_eq!(app.cursor, 2);
}

#[test]
fn command_submission_restores_visual_and_side_panel_modes() {
    let mut visual = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
    visual.cursor = 1;
    visual.toggle_visual();
    visual.move_horizontal(2);

    type_command(&mut visual, "hash crc32");

    assert_eq!(visual.mode, Mode::Visual);
    assert_eq!(visual.selection_range(), Some((1, 3)));
    assert!(visual.status_message.contains("crc32"));

    let mut panel = gif_inspector_app();
    type_command(&mut panel, "hash crc32");

    assert_eq!(panel.mode, Mode::SidePanel);
    assert_eq!(
        panel.active_side_panel,
        crate::app::SidePanelKind::Inspector
    );
    assert_eq!(panel.cursor, 3);
    assert!(panel.status_message.contains("crc32"));
}
