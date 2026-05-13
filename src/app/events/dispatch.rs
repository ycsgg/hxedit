use super::*;

impl App {
    pub(super) fn dispatch_action(&mut self, action: Action) -> HxResult<()> {
        if self.handle_navigation_action(action) {
            return Ok(());
        }
        if self.handle_command_action(action)? {
            return Ok(());
        }
        if self.handle_side_panel_action(action)? {
            return Ok(());
        }
        self.handle_editor_action(action)
    }

    pub(super) fn handle_navigation_action(&mut self, action: Action) -> bool {
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
                if self.mode.is_side_panel() && self.active_side_panel == SidePanelKind::Symbol {
                    self.move_symbol_selection(-(self.symbol_list_visible_rows() as i64));
                } else if self.mode.is_side_panel() && self.active_side_panel == SidePanelKind::Data
                {
                    self.scroll_data_panel(-(self.side_panel_visible_rows() as i64));
                } else if self.mode.is_side_panel() && self.active_side_panel == SidePanelKind::Diff
                {
                    self.move_diff_selection(-(self.side_panel_visible_rows() as i64));
                } else {
                    self.move_vertical(-(self.view_rows as i64));
                }
                true
            }
            Action::PageDown => {
                if self.mode.is_side_panel() && self.active_side_panel == SidePanelKind::Symbol {
                    self.move_symbol_selection(self.symbol_list_visible_rows() as i64);
                } else if self.mode.is_side_panel() && self.active_side_panel == SidePanelKind::Data
                {
                    self.scroll_data_panel(self.side_panel_visible_rows() as i64);
                } else if self.mode.is_side_panel() && self.active_side_panel == SidePanelKind::Diff
                {
                    self.move_diff_selection(self.side_panel_visible_rows() as i64);
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

    pub(super) fn handle_editor_action(&mut self, action: Action) -> HxResult<()> {
        match action {
            Action::ToggleVisual => {
                self.toggle_visual();
                Ok(())
            }
            Action::BeginDisasmEdit => self.begin_disasm_edit(),
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
            Action::ToggleSidePanel => {
                self.toggle_side_panel();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn handle_command_action(&mut self, action: Action) -> HxResult<bool> {
        if matches!(self.mode, Mode::DisasmEdit) {
            return match action {
                Action::CommandChar(c) => {
                    self.insert_disasm_char(c);
                    Ok(true)
                }
                Action::CommandLeft => {
                    self.move_disasm_cursor(true);
                    Ok(true)
                }
                Action::CommandRight => {
                    self.move_disasm_cursor(false);
                    Ok(true)
                }
                Action::CommandHome => {
                    self.set_disasm_cursor(true);
                    Ok(true)
                }
                Action::CommandEnd => {
                    self.set_disasm_cursor(false);
                    Ok(true)
                }
                Action::CommandDelete => {
                    self.delete_disasm_char();
                    Ok(true)
                }
                Action::CommandBackspace => {
                    self.backspace_disasm_char();
                    Ok(true)
                }
                Action::CommandSubmit => {
                    self.submit_disasm_edit()?;
                    Ok(true)
                }
                Action::CommandCancel => {
                    self.cancel_disasm_edit();
                    Ok(true)
                }
                _ => Ok(false),
            };
        }

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

    pub(super) fn handle_side_panel_action(&mut self, action: Action) -> HxResult<bool> {
        match action {
            Action::SidePanelUp => {
                match self.active_side_panel {
                    SidePanelKind::Inspector => self.move_inspector_selection(true),
                    SidePanelKind::Symbol => self.move_symbol_selection(-1),
                    SidePanelKind::Data => self.move_data_panel_selection(-1),
                    SidePanelKind::Diff => self.move_diff_selection(-1),
                }
                Ok(true)
            }
            Action::SidePanelDown => {
                match self.active_side_panel {
                    SidePanelKind::Inspector => self.move_inspector_selection(false),
                    SidePanelKind::Symbol => self.move_symbol_selection(1),
                    SidePanelKind::Data => self.move_data_panel_selection(1),
                    SidePanelKind::Diff => self.move_diff_selection(1),
                }
                Ok(true)
            }
            Action::SidePanelEnter => {
                match self.active_side_panel {
                    SidePanelKind::Inspector => self.handle_inspector_enter()?,
                    SidePanelKind::Symbol => self.navigate_to_selected_symbol()?,
                    SidePanelKind::Data => self.move_data_panel_selection(0),
                    SidePanelKind::Diff => self.navigate_to_selected_diff_hunk()?,
                }
                Ok(true)
            }
            Action::SidePanelToggleCollapse => {
                if self.active_side_panel != SidePanelKind::Inspector {
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
            Action::SidePanelChar(c) => {
                if self.active_side_panel == SidePanelKind::Inspector {
                    self.insert_inspector_char(c);
                } else if c == 't' {
                    self.toggle_side_panel();
                }
                Ok(true)
            }
            Action::SidePanelBackspace => {
                if self.active_side_panel == SidePanelKind::Inspector {
                    self.backspace_inspector_char();
                }
                Ok(true)
            }
            Action::SidePanelLeft => {
                if self.active_side_panel == SidePanelKind::Inspector {
                    self.move_inspector_cursor(true);
                }
                Ok(true)
            }
            Action::SidePanelRight => {
                if self.active_side_panel == SidePanelKind::Inspector {
                    self.move_inspector_cursor(false);
                }
                Ok(true)
            }
            Action::SidePanelHome => {
                if self.active_side_panel == SidePanelKind::Inspector {
                    self.set_inspector_cursor(true);
                }
                Ok(true)
            }
            Action::SidePanelEnd => {
                if self.active_side_panel == SidePanelKind::Inspector {
                    self.set_inspector_cursor(false);
                }
                Ok(true)
            }
            Action::SidePanelDelete => {
                if self.active_side_panel == SidePanelKind::Inspector {
                    self.delete_inspector_char();
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
