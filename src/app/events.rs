use crate::action::Action;
use crate::app::text_cursor::{
    backspace_char_before_cursor, delete_char_at_cursor, insert_char_at_cursor, move_cursor_end,
    move_cursor_home, move_cursor_left, move_cursor_right,
};
use crate::app::App;
use crate::error::{HxError, HxResult};
use crate::format::parse::InspectorRow;
use crate::mode::Mode;
use crate::mode::NibblePhase;

impl App {
    pub(crate) fn handle_action(&mut self, action: Action) {
        let result = self.dispatch_action(action);
        self.finish_action(action, result);
    }
}

impl App {
    fn dispatch_action(&mut self, action: Action) -> HxResult<()> {
        if self.handle_navigation_action(action) {
            return Ok(());
        }
        if self.handle_command_action(action)? {
            return Ok(());
        }
        if self.handle_inspector_action(action)? {
            return Ok(());
        }
        self.handle_editor_action(action)
    }

    fn handle_navigation_action(&mut self, action: Action) -> bool {
        match action {
            Action::MoveLeft => {
                self.move_horizontal(-1);
                true
            }
            Action::MoveRight => {
                self.move_horizontal(1);
                true
            }
            Action::MoveUp => {
                self.move_vertical(-1);
                true
            }
            Action::MoveDown => {
                self.move_vertical(1);
                true
            }
            Action::PageUp => {
                if self.mode.is_inspector()
                    && matches!(self.side_panel, Some(crate::app::SidePanel::Symbol(_)))
                {
                    self.move_symbol_selection(-(self.symbol_list_visible_rows() as i64));
                } else if self.mode.is_inspector()
                    && matches!(self.side_panel, Some(crate::app::SidePanel::Data(_)))
                {
                    self.scroll_data_panel(-(self.inspector_visible_rows() as i64));
                } else {
                    self.move_vertical(-(self.view_rows as i64));
                }
                true
            }
            Action::PageDown => {
                if self.mode.is_inspector()
                    && matches!(self.side_panel, Some(crate::app::SidePanel::Symbol(_)))
                {
                    self.move_symbol_selection(self.symbol_list_visible_rows() as i64);
                } else if self.mode.is_inspector()
                    && matches!(self.side_panel, Some(crate::app::SidePanel::Data(_)))
                {
                    self.scroll_data_panel(self.inspector_visible_rows() as i64);
                } else {
                    self.move_vertical(self.view_rows as i64);
                }
                true
            }
            Action::RowStart => {
                self.move_row_edge(false);
                true
            }
            Action::RowEnd => {
                self.move_row_edge(true);
                true
            }
            _ => false,
        }
    }

    fn handle_editor_action(&mut self, action: Action) -> HxResult<()> {
        match action {
            Action::ToggleVisual => {
                self.toggle_visual();
                Ok(())
            }
            Action::EnterInsert => self.enter_hex_mode(true),
            Action::EnterReplace => self.enter_hex_mode(false),
            Action::EnterCommand => {
                self.enter_command_mode();
                Ok(())
            }
            Action::LeaveMode => {
                self.leave_mode();
                Ok(())
            }
            Action::DeleteByte => self.delete_at_cursor_or_selection(),
            Action::SearchNext => self.repeat_search(crate::app::SearchDirection::Forward),
            Action::SearchPrev => self.repeat_search(crate::app::SearchDirection::Backward),
            Action::Undo(steps) => self.handle_undo_action(steps),
            Action::Redo(steps) => self.handle_redo_action(steps),
            Action::EditHex(value) => self.handle_edit_hex_action(value),
            Action::EditBackspace => self.edit_backspace(),
            Action::ForceQuit => {
                self.should_quit = true;
                Ok(())
            }
            Action::ToggleInspector => {
                self.toggle_inspector_mode();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn handle_command_action(&mut self, action: Action) -> HxResult<bool> {
        match action {
            Action::CommandChar(c) => {
                self.insert_command_char(c);
                Ok(true)
            }
            Action::CommandLeft => {
                self.move_command_cursor_left();
                Ok(true)
            }
            Action::CommandRight => {
                self.move_command_cursor_right();
                Ok(true)
            }
            Action::CommandHome => {
                self.move_command_cursor_home();
                Ok(true)
            }
            Action::CommandEnd => {
                self.move_command_cursor_end();
                Ok(true)
            }
            Action::CommandDelete => {
                self.delete_command_char();
                Ok(true)
            }
            Action::CommandBackspace => {
                self.backspace_command_char();
                Ok(true)
            }
            Action::CommandHistoryPrev => {
                self.command_history_prev();
                Ok(true)
            }
            Action::CommandHistoryNext => {
                self.command_history_next();
                Ok(true)
            }
            Action::CommandSubmit => {
                self.submit_command()?;
                Ok(true)
            }
            Action::CommandCancel => {
                self.cancel_command_input();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn handle_inspector_action(&mut self, action: Action) -> HxResult<bool> {
        match action {
            Action::InspectorUp => {
                // Check if we're in symbol panel
                if matches!(self.side_panel, Some(crate::app::SidePanel::Symbol(_))) {
                    self.move_symbol_selection(-1);
                } else {
                    self.move_inspector_selection(true);
                }
                Ok(true)
            }
            Action::InspectorDown => {
                // Check if we're in symbol panel
                if matches!(self.side_panel, Some(crate::app::SidePanel::Symbol(_))) {
                    self.move_symbol_selection(1);
                } else {
                    self.move_inspector_selection(false);
                }
                Ok(true)
            }
            Action::InspectorEnter => {
                // Check if we're in symbol panel
                if matches!(self.side_panel, Some(crate::app::SidePanel::Symbol(_))) {
                    self.navigate_to_selected_symbol()?;
                } else {
                    self.handle_inspector_enter()?;
                }
                Ok(true)
            }
            Action::InspectorToggleCollapse => {
                if matches!(self.side_panel, Some(crate::app::SidePanel::Symbol(_))) {
                    return Ok(true);
                }
                // While a field is being edited the space key should reach the
                // edit buffer, not toggle collapse. Redirect to Char(' ').
                if self
                    .inspector()
                    .is_some_and(|inspector| inspector.editing.is_some())
                {
                    self.insert_inspector_char(' ');
                } else {
                    self.toggle_inspector_collapse();
                }
                Ok(true)
            }
            Action::InspectorChar(c) => {
                self.insert_inspector_char(c);
                Ok(true)
            }
            Action::InspectorBackspace => {
                self.backspace_inspector_char();
                Ok(true)
            }
            Action::InspectorLeft => {
                self.move_inspector_cursor(true);
                Ok(true)
            }
            Action::InspectorRight => {
                self.move_inspector_cursor(false);
                Ok(true)
            }
            Action::InspectorHome => {
                self.set_inspector_cursor(true);
                Ok(true)
            }
            Action::InspectorEnd => {
                self.set_inspector_cursor(false);
                Ok(true)
            }
            Action::InspectorDelete => {
                self.delete_inspector_char();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn finish_action(&mut self, action: Action, result: HxResult<()>) {
        match result {
            Ok(()) => {
                self.ensure_cursor_visible();
                self.sync_inspector_to_cursor();
                self.refresh_data_panel();
                if !is_command_edit_action(action) {
                    self.clear_error_if_command_done();
                }
            }
            Err(err) => {
                self.set_error_status(err.to_string());
            }
        }
    }

    fn enter_hex_mode(&mut self, insert: bool) -> HxResult<()> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        if insert && matches!(self.main_view, crate::app::MainView::Disassembly(_)) {
            return Err(HxError::DisassemblyUnavailable(
                "view is overwrite-only; use :dis off for layout-changing edits".to_owned(),
            ));
        }
        self.mode = if insert {
            Mode::InsertHex { pending: None }
        } else {
            Mode::EditHex {
                phase: NibblePhase::High,
            }
        };
        Ok(())
    }

    fn enter_command_mode(&mut self) {
        let return_mode = if matches!(self.mode, Mode::InsertHex { .. }) {
            self.commit_pending_insert();
            Mode::Normal
        } else {
            self.mode
        };
        self.command_return_mode = Some(return_mode);
        self.mode = Mode::Command;
        self.reset_command_history_navigation();
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
    }

    fn handle_undo_action(&mut self, steps: usize) -> HxResult<()> {
        if self.undo_pending_insert()? {
            if steps > 1 {
                self.undo(steps - 1, true)
            } else {
                Ok(())
            }
        } else {
            self.undo(steps, true)
        }
    }

    fn handle_redo_action(&mut self, steps: usize) -> HxResult<()> {
        self.redo(steps, true)
    }

    fn handle_edit_hex_action(&mut self, value: u8) -> HxResult<()> {
        match self.mode {
            Mode::InsertHex { .. } => self.insert_nibble(value),
            _ => self.edit_nibble(value),
        }
    }

    fn move_inspector_selection(&mut self, upward: bool) {
        let Some(inspector) = self.inspector_mut() else {
            return;
        };
        if inspector.editing.is_some() {
            return;
        }

        let mut target = inspector.selected_row;
        loop {
            if upward {
                if target == 0 {
                    break;
                }
                target -= 1;
            } else {
                target += 1;
                if target >= inspector.rows.len() {
                    break;
                }
            }

            if inspector
                .rows
                .get(target)
                .is_some_and(crate::app::inspector_state::is_selectable)
            {
                inspector.selected_row = target;
                break;
            }
        }

        self.sync_cursor_to_inspector();
    }

    fn handle_inspector_enter(&mut self) -> HxResult<()> {
        let Some(inspector) = self.inspector_mut() else {
            return Ok(());
        };
        if inspector.editing.is_some() {
            return self.submit_inspector_edit();
        }

        let Some(row) = inspector.rows.get(inspector.selected_row).cloned() else {
            return Ok(());
        };

        match row {
            InspectorRow::Field {
                editable: true,
                display,
                ..
            } => {
                inspector.editing = Some(crate::app::InspectorEdit {
                    row_index: inspector.selected_row,
                    buffer: display.clone(),
                    cursor_pos: display.len(),
                });
                self.mode = Mode::InspectorEdit;
                if let Some(warning) = self.inspector_edit_warning() {
                    self.set_warning_status(warning);
                }
            }
            InspectorRow::Field {
                editable: false,
                name,
                ..
            } => {
                let format_name = inspector.format_name.clone();
                self.set_info_status(self.inspector_read_only_message(&format_name, &name));
            }
            InspectorRow::Header {
                has_children: true, ..
            } => {
                // Header Enter = toggle — consistent with ImHex / file tree UIs.
                self.toggle_inspector_collapse();
            }
            InspectorRow::Header { .. } => {}
        }
        Ok(())
    }

    fn insert_inspector_char(&mut self, c: char) {
        if let Some(inspector) = self.inspector_mut() {
            if let Some(edit) = inspector.editing.as_mut() {
                insert_char_at_cursor(&mut edit.buffer, &mut edit.cursor_pos, c);
            } else if c == 't' {
                self.toggle_inspector_mode();
            }
        }
    }

    fn backspace_inspector_char(&mut self) {
        if let Some(edit) = self.inspector_edit_mut() {
            backspace_char_before_cursor(&mut edit.buffer, &mut edit.cursor_pos);
        }
    }

    fn move_inspector_cursor(&mut self, left: bool) {
        if let Some(edit) = self.inspector_edit_mut() {
            if left {
                move_cursor_left(&edit.buffer, &mut edit.cursor_pos);
            } else {
                move_cursor_right(&edit.buffer, &mut edit.cursor_pos);
            }
        }
    }

    fn set_inspector_cursor(&mut self, home: bool) {
        if let Some(edit) = self.inspector_edit_mut() {
            if home {
                move_cursor_home(&mut edit.cursor_pos);
            } else {
                move_cursor_end(&edit.buffer, &mut edit.cursor_pos);
            }
        }
    }

    fn delete_inspector_char(&mut self) {
        if let Some(edit) = self.inspector_edit_mut() {
            delete_char_at_cursor(&mut edit.buffer, edit.cursor_pos);
        }
    }

    fn inspector_edit_mut(&mut self) -> Option<&mut crate::app::InspectorEdit> {
        self.inspector_mut()?.editing.as_mut()
    }
}

fn is_command_edit_action(action: Action) -> bool {
    matches!(
        action,
        Action::CommandChar(_)
            | Action::CommandLeft
            | Action::CommandRight
            | Action::CommandHome
            | Action::CommandEnd
            | Action::CommandDelete
            | Action::CommandBackspace
            | Action::CommandHistoryPrev
            | Action::CommandHistoryNext
    )
}
#[cfg(test)]
mod tests {
    use std::fs;

    use ratatui::layout::Rect;
    use tempfile::tempdir;

    use super::*;
    use crate::action::Action;
    use crate::cli::Cli;
    use crate::format::parse::{FieldValue, StructValue};
    use crate::format::types::{FieldDef, FieldType};

    fn app_with_len(len: usize) -> App {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, vec![0_u8; len]).unwrap();
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
        app.view_rows = 4;
        app
    }

    fn app_with_inspector_field_for(format_name: &str) -> App {
        app_with_inspector_field_editable(format_name, true)
    }

    fn app_with_inspector_field_editable(format_name: &str, editable: bool) -> App {
        let mut app = app_with_len(4);
        let field = FieldDef {
            name: "entry".to_owned(),
            offset: 0,
            field_type: FieldType::U8,
            description: String::new(),
            editable,
        };
        let structs = vec![StructValue {
            name: "Header".to_owned(),
            base_offset: 0,
            fields: vec![FieldValue {
                def: field,
                abs_offset: 0,
                raw_bytes: vec![0],
                display: "0x00".to_owned(),
                size: 1,
            }],
            children: Vec::new(),
        }];
        let collapsed_nodes = std::collections::BTreeSet::new();
        let rows = crate::format::parse::flatten(&structs, &collapsed_nodes);
        app.show_inspector = true;
        app.side_panel = Some(crate::app::SidePanel::Inspector(
            crate::app::InspectorState {
                format_name: format_name.to_owned(),
                structs,
                rows,
                scroll_offset: 0,
                selected_row: 1,
                editing: None,
                collapsed_nodes,
            },
        ));
        app.mode = Mode::Inspector;
        app
    }

    fn app_with_inspector_field() -> App {
        app_with_inspector_field_for("TEST")
    }

    fn type_command(app: &mut App, input: &str) {
        for ch in input.chars() {
            app.handle_action(Action::CommandChar(ch));
        }
    }

    #[test]
    fn command_cursor_navigation_and_editing() {
        // Move and insert in middle
        let mut app = app_with_len(4);
        app.handle_action(Action::EnterCommand);
        app.handle_action(Action::CommandChar('a'));
        app.handle_action(Action::CommandChar('b'));
        app.handle_action(Action::CommandChar('c'));
        app.handle_action(Action::CommandLeft);
        app.handle_action(Action::CommandLeft);
        app.handle_action(Action::CommandChar('X'));
        assert_eq!(app.command_buffer, "aXbc");
        assert_eq!(app.command_cursor_pos, 2);

        // Backspace respects cursor position (separate test)
        let mut app2 = app_with_len(4);
        app2.handle_action(Action::EnterCommand);
        app2.handle_action(Action::CommandChar('a'));
        app2.handle_action(Action::CommandChar('b'));
        app2.handle_action(Action::CommandChar('c'));
        app2.handle_action(Action::CommandLeft);
        app2.handle_action(Action::CommandBackspace);
        assert_eq!(app2.command_buffer, "ac");
        assert_eq!(app2.command_cursor_pos, 1);

        // Delete, home, end
        let mut app3 = app_with_len(4);
        app3.handle_action(Action::EnterCommand);
        type_command(&mut app3, "abcd");
        app3.handle_action(Action::CommandHome);
        assert_eq!(app3.command_cursor_pos, 0);
        app3.handle_action(Action::CommandRight);
        app3.handle_action(Action::CommandRight);
        app3.handle_action(Action::CommandDelete);
        assert_eq!(app3.command_buffer, "abd");
        app3.handle_action(Action::CommandEnd);
        assert_eq!(app3.command_cursor_pos, app3.command_buffer.len());
    }

    #[test]
    fn inspector_enter_escape_and_format_warnings() {
        let mut app = app_with_inspector_field();
        app.handle_action(Action::InspectorEnter);
        assert_eq!(app.mode, Mode::InspectorEdit);
        assert!(app
            .inspector()
            .and_then(|inspector| inspector.editing.as_ref())
            .is_some());

        app.handle_action(Action::LeaveMode);
        assert_eq!(app.mode, Mode::Inspector);
        assert!(app
            .inspector()
            .and_then(|inspector| inspector.editing.as_ref())
            .is_none());

        // PNG format warning
        let mut app_png = app_with_inspector_field_for("PNG");
        app_png.handle_action(Action::InspectorEnter);
        assert_eq!(app_png.status_level, crate::app::StatusLevel::Warning);
        assert!(app_png.status_message.contains("PNG inspector edits"));

        // GZIP format warning
        let mut app_gz = app_with_inspector_field_for("GZIP");
        app_gz.handle_action(Action::InspectorEnter);
        assert!(app_gz.status_message.contains("GZIP inspector edits"));

        // TAR format warning
        let mut app_tar = app_with_inspector_field_for("TAR");
        app_tar.handle_action(Action::InspectorEnter);
        assert!(app_tar.status_message.contains("TAR inspector edits"));

        // JPEG format warning
        let mut app_jpg = app_with_inspector_field_for("JPEG");
        app_jpg.handle_action(Action::InspectorEnter);
        assert!(app_jpg.status_message.contains("JPEG inspector edits"));
    }

    #[test]
    fn inspector_warns_when_editing_png_field() {
        let mut app = app_with_inspector_field_for("PNG");

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::InspectorEdit);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("PNG inspector edits"));
    }

    #[test]
    fn inspector_warns_when_editing_gzip_field() {
        let mut app = app_with_inspector_field_for("GZIP");

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::InspectorEdit);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("GZIP inspector edits"));
    }

    #[test]
    fn inspector_warns_when_editing_gif_field() {
        let mut app = app_with_inspector_field_for("GIF");

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::InspectorEdit);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("GIF inspector edits"));
    }

    #[test]
    fn inspector_warns_when_editing_bmp_field() {
        let mut app = app_with_inspector_field_for("BMP");

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::InspectorEdit);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("BMP inspector edits"));
    }

    #[test]
    fn inspector_warns_when_editing_wav_field() {
        let mut app = app_with_inspector_field_for("WAV");

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::InspectorEdit);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("WAV inspector edits"));
    }

    #[test]
    fn inspector_warns_when_editing_tar_field() {
        let mut app = app_with_inspector_field_for("TAR");

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::InspectorEdit);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("TAR inspector edits"));
    }

    #[test]
    fn inspector_warns_when_editing_jpeg_field() {
        let mut app = app_with_inspector_field_for("JPEG");

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::InspectorEdit);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("JPEG inspector edits"));
    }

    #[test]
    fn hidden_inspector_focus_falls_back_to_normal_mode() {
        let mut app = app_with_inspector_field();
        app.last_columns = Some(crate::view::layout::MainColumns {
            main_pane_kind: crate::view::layout::MainPaneKind::Hex,
            gutter: Rect::new(0, 0, 4, 4),
            sep1: Rect::new(4, 0, 1, 4),
            hex: Rect::new(5, 0, 20, 4),
            sep2: Rect::new(25, 0, 1, 4),
            ascii: Rect::new(26, 0, 10, 4),
            sep3: None,
            inspector: None,
        });
        app.ensure_inspector_mode_visible();
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.status_message.contains("too narrow"));

        // Entering inspector from normal mode warns when hidden
        let mut app2 = app_with_inspector_field();
        app2.mode = Mode::Normal;
        app2.last_columns = app.last_columns;
        app2.handle_action(Action::ToggleInspector);
        assert_eq!(app2.mode, Mode::Normal);
        assert!(app2.show_inspector);
        assert!(app2.status_message.contains("too narrow"));

        // Entering inspector succeeds when width is sufficient
        let mut app3 = app_with_inspector_field();
        app3.mode = Mode::Normal;
        app3.last_columns = Some(crate::view::layout::MainColumns {
            main_pane_kind: crate::view::layout::MainPaneKind::Hex,
            gutter: Rect::new(0, 0, 8, 4),
            sep1: Rect::new(8, 0, 1, 4),
            hex: Rect::new(9, 0, 90, 4),
            sep2: Rect::new(99, 0, 1, 4),
            ascii: Rect::new(100, 0, 40, 4),
            sep3: None,
            inspector: None,
        });
        app3.handle_action(Action::ToggleInspector);
        assert_eq!(app3.mode, Mode::Inspector);
        assert!(!app3.status_message.contains("too narrow"));

        // No detected format stays in normal with hint
        let mut app4 = app_with_len(8);
        app4.mode = Mode::Normal;
        app4.last_columns = app3.last_columns;
        app4.handle_action(Action::ToggleInspector);
        assert_eq!(app4.mode, Mode::Normal);
        assert!(app4.status_message.contains("no format detected"));
        assert!(app4
            .status_message
            .contains("ELF / PNG / ZIP / GZIP / GIF / BMP / WAV / TAR / JPEG"));

        // View-only inspector reports read-only mode
        let mut app5 = app_with_inspector_field_editable("TEST", false);
        app5.mode = Mode::Normal;
        app5.last_columns = app3.last_columns;
        app5.handle_action(Action::ToggleInspector);
        assert_eq!(app5.mode, Mode::Inspector);
        assert!(app5.status_message.contains("view-only"));

        // Enter on read-only field reports reason
        let mut app6 = app_with_inspector_field_editable("TEST", false);
        app6.handle_action(Action::InspectorEnter);
        assert_eq!(app6.mode, Mode::Inspector);
        assert!(app6.status_message.contains("read-only"));
    }

    #[test]
    fn entering_inspector_without_detected_format_stays_in_normal_with_hint() {
        let mut app = app_with_len(8);
        app.mode = Mode::Normal;
        app.last_columns = Some(crate::view::layout::MainColumns {
            main_pane_kind: crate::view::layout::MainPaneKind::Hex,
            gutter: Rect::new(0, 0, 8, 4),
            sep1: Rect::new(8, 0, 1, 4),
            hex: Rect::new(9, 0, 90, 4),
            sep2: Rect::new(99, 0, 1, 4),
            ascii: Rect::new(100, 0, 40, 4),
            sep3: None,
            inspector: None,
        });

        app.handle_action(Action::ToggleInspector);

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.show_inspector);
        assert_eq!(app.status_level, crate::app::StatusLevel::Warning);
        assert!(app.status_message.contains("no format detected"));
        assert!(app
            .status_message
            .contains("ELF / PNG / ZIP / GZIP / GIF / BMP / WAV / TAR / JPEG"));
    }

    #[test]
    fn entering_view_only_inspector_reports_read_only_mode() {
        let mut app = app_with_inspector_field_editable("TEST", false);
        app.mode = Mode::Normal;
        app.last_columns = Some(crate::view::layout::MainColumns {
            main_pane_kind: crate::view::layout::MainPaneKind::Hex,
            gutter: Rect::new(0, 0, 8, 4),
            sep1: Rect::new(8, 0, 1, 4),
            hex: Rect::new(9, 0, 90, 4),
            sep2: Rect::new(99, 0, 1, 4),
            ascii: Rect::new(100, 0, 40, 4),
            sep3: None,
            inspector: None,
        });

        app.handle_action(Action::ToggleInspector);

        assert_eq!(app.mode, Mode::Inspector);
        assert_eq!(app.status_level, crate::app::StatusLevel::Info);
        assert!(app.status_message.contains("view-only"));
    }

    #[test]
    fn inspector_enter_on_read_only_field_reports_reason() {
        let mut app = app_with_inspector_field_editable("TEST", false);

        app.handle_action(Action::InspectorEnter);

        assert_eq!(app.mode, Mode::Inspector);
        assert_eq!(app.status_level, crate::app::StatusLevel::Info);
        assert!(app.status_message.contains("read-only"));
        assert!(app.status_message.contains("entry"));
    }

    #[test]
    fn command_mode_success_and_history() {
        // Failed command keeps buffer for editing
        let mut app = app_with_len(1);
        app.mode = Mode::EditHex {
            phase: NibblePhase::High,
        };
        app.edit_nibble(0xa).unwrap();
        app.mode = Mode::Normal;
        app.handle_action(Action::EnterCommand);
        app.handle_action(Action::CommandChar('q'));
        app.handle_action(Action::CommandSubmit);
        assert_eq!(app.mode, Mode::Command);
        assert_eq!(app.command_buffer, "q");
        assert_eq!(app.status_level, crate::app::StatusLevel::Error);
        assert!(app.status_message.contains("unsaved changes"));

        // Success clears buffer and browses history
        let mut app2 = app_with_len(16);
        app2.handle_action(Action::EnterCommand);
        type_command(&mut app2, "goto 0x4");
        app2.handle_action(Action::CommandSubmit);
        app2.handle_action(Action::EnterCommand);
        assert!(app2.command_buffer.is_empty());
        type_command(&mut app2, "goto 0x1");
        app2.handle_action(Action::CommandSubmit);
        app2.handle_action(Action::EnterCommand);
        assert!(app2.command_buffer.is_empty());
        type_command(&mut app2, "und");
        app2.handle_action(Action::CommandHistoryPrev);
        assert_eq!(app2.command_buffer, "goto 0x1");
        app2.handle_action(Action::CommandHistoryPrev);
        assert_eq!(app2.command_buffer, "goto 0x4");
        app2.handle_action(Action::CommandHistoryNext);
        assert_eq!(app2.command_buffer, "goto 0x1");
        app2.handle_action(Action::CommandHistoryNext);
        assert_eq!(app2.command_buffer, "und");
    }

    fn app_with_nested_inspector() -> App {
        let mut app = app_with_len(8);
        let parent_field = FieldDef {
            name: "parent_byte".to_owned(),
            offset: 0,
            field_type: FieldType::U8,
            description: String::new(),
            editable: true,
        };
        let child_field = FieldDef {
            name: "child_byte".to_owned(),
            offset: 0,
            field_type: FieldType::U8,
            description: String::new(),
            editable: true,
        };
        let structs = vec![StructValue {
            name: "Parent".to_owned(),
            base_offset: 0,
            fields: vec![FieldValue {
                def: parent_field,
                abs_offset: 0,
                raw_bytes: vec![0],
                display: "0x00".to_owned(),
                size: 1,
            }],
            children: vec![StructValue {
                name: "Child".to_owned(),
                base_offset: 4,
                fields: vec![FieldValue {
                    def: child_field,
                    abs_offset: 4,
                    raw_bytes: vec![0],
                    display: "0x00".to_owned(),
                    size: 1,
                }],
                children: Vec::new(),
            }],
        }];
        let collapsed_nodes = std::collections::BTreeSet::new();
        let rows = crate::format::parse::flatten(&structs, &collapsed_nodes);
        app.show_inspector = true;
        app.side_panel = Some(crate::app::SidePanel::Inspector(
            crate::app::InspectorState {
                format_name: "TEST".to_owned(),
                structs,
                rows,
                scroll_offset: 0,
                selected_row: 0,
                editing: None,
                collapsed_nodes,
            },
        ));
        app.mode = Mode::Inspector;
        app
    }

    #[test]
    fn inspector_collapse_toggle_and_navigation() {
        let mut app = app_with_nested_inspector();
        // Layout: [0]=Parent header, [1]=parent_byte, [2]=Child header, [3]=child_byte
        assert_eq!(app.inspector().unwrap().rows.len(), 4);
        assert_eq!(app.inspector().unwrap().selected_row, 0); // Parent header

        // Collapse Parent — fields AND child go away.
        app.handle_action(Action::InspectorToggleCollapse);
        assert_eq!(app.inspector().unwrap().rows.len(), 1);
        assert!(matches!(
            app.inspector().unwrap().rows[0],
            InspectorRow::Header {
                collapsed: true,
                ..
            }
        ));
        assert!(app
            .inspector()
            .unwrap()
            .collapsed_nodes
            .contains(&vec![("Parent".to_owned(), 0)]));

        // Toggle twice restores original rows
        app.handle_action(Action::InspectorToggleCollapse);
        assert_eq!(app.inspector().unwrap().rows.len(), 4);
        assert!(app.inspector().unwrap().collapsed_nodes.is_empty());

        // Header Enter toggles collapse when not editing
        app.handle_action(Action::InspectorEnter);
        assert!(app
            .inspector()
            .unwrap()
            .collapsed_nodes
            .contains(&vec![("Parent".to_owned(), 0)]));
        assert_eq!(app.mode, Mode::Inspector);

        // Navigation stops on collapsible header
        let mut app2 = app_with_nested_inspector();
        assert_eq!(app2.inspector().unwrap().selected_row, 0);
        app2.handle_action(Action::InspectorDown);
        assert_eq!(app2.inspector().unwrap().selected_row, 1);
        app2.handle_action(Action::InspectorDown);
        assert_eq!(app2.inspector().unwrap().selected_row, 2);
        assert!(matches!(
            app2.inspector().unwrap().rows[2],
            InspectorRow::Header { .. }
        ));

        // Space during edit inserts into buffer instead of toggling
        let mut app3 = app_with_inspector_field();
        app3.handle_action(Action::InspectorEnter);
        let before_buf = app3
            .inspector()
            .and_then(|i| i.editing.as_ref())
            .map(|e| e.buffer.clone())
            .unwrap();
        app3.handle_action(Action::InspectorToggleCollapse);
        let after = app3
            .inspector()
            .and_then(|i| i.editing.as_ref())
            .map(|e| e.buffer.clone())
            .unwrap();
        assert_eq!(after.len(), before_buf.len() + 1);
        assert!(after.ends_with(' '));

        // Toggle on non-header row is noop
        let mut app4 = app_with_nested_inspector();
        app4.handle_action(Action::InspectorDown);
        assert_eq!(app4.inspector().unwrap().selected_row, 1);
        app4.handle_action(Action::InspectorToggleCollapse);
        assert!(app4.inspector().unwrap().collapsed_nodes.is_empty());
    }
}
