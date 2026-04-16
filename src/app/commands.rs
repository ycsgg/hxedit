use crate::app::{App, SearchDirection, SearchKind, SearchState};
use crate::commands::parser::parse_command;
use crate::commands::types::{Command, GotoTarget};
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

impl App {
    pub(crate) fn submit_command(&mut self) -> HxResult<()> {
        let return_mode = self.command_return_mode.unwrap_or(Mode::Normal);
        let command = parse_command(&self.command_buffer)?;
        self.execute_command(command)?;
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        if matches!(self.mode, Mode::Command) {
            self.mode = self.normalize_mode(return_mode);
        }
        self.command_return_mode = None;
        Ok(())
    }

    pub(crate) fn execute_command(&mut self, command: Command) -> HxResult<()> {
        match command {
            Command::Quit { force } => self.execute_quit_command(force),
            Command::Write { path } => self.execute_write_command(path, false),
            Command::WriteQuit { path } => self.execute_write_command(path, true),
            Command::Goto { target } => self.execute_goto_command(target),
            Command::Undo { steps } => self.undo(steps, false),
            Command::Paste {
                raw,
                preview,
                limit,
            } => self.execute_paste_command(raw, preview, limit, false),
            Command::PasteInsert {
                raw,
                preview,
                limit,
            } => self.execute_paste_command(raw, preview, limit, true),
            Command::Copy { format, display } => self.copy_selection(format, display),
            Command::Inspector => {
                self.execute_inspector_command();
                Ok(())
            }
            Command::Format { name } => {
                self.execute_format_command(name);
                Ok(())
            }
            Command::SearchAscii { pattern, backward } => {
                self.execute_search_command(SearchKind::Ascii, pattern, backward)
            }
            Command::SearchHex { pattern, backward } => {
                self.execute_search_command(SearchKind::Hex, pattern, backward)
            }
        }
    }

    fn execute_quit_command(&mut self, force: bool) -> HxResult<()> {
        if self.document.is_dirty() && !force {
            return Err(HxError::DirtyQuit);
        }
        self.should_quit = true;
        Ok(())
    }

    fn execute_write_command(
        &mut self,
        path: Option<std::path::PathBuf>,
        should_quit: bool,
    ) -> HxResult<()> {
        let (saved, profile) = self.document.save(path)?;
        self.undo_stack.clear();
        self.cursor = self.clamp_offset(self.cursor);
        self.refresh_inspector();
        self.set_info_status(format!("wrote {} [{}]", saved.display(), profile));
        self.should_quit = should_quit;
        Ok(())
    }

    fn execute_goto_command(&mut self, target: GotoTarget) -> HxResult<()> {
        let offset = self.resolve_goto_target(target)?;
        self.cursor = self.document.goto(offset)?;
        self.set_info_status(format!("goto 0x{:x}", self.cursor));
        Ok(())
    }

    fn resolve_goto_target(&self, target: GotoTarget) -> HxResult<u64> {
        match target {
            GotoTarget::Absolute(offset) => Ok(offset),
            GotoTarget::End => {
                if self.document.is_empty() {
                    Ok(0)
                } else {
                    Ok(self.document.len() - 1)
                }
            }
            GotoTarget::Relative(delta) => {
                let current = i64::try_from(self.cursor)
                    .map_err(|_| HxError::InvalidOffset(delta.to_string()))?;
                let target = current.saturating_add(delta);
                u64::try_from(target).map_err(|_| HxError::OffsetOutOfRange)
            }
        }
    }

    fn execute_paste_command(
        &mut self,
        raw: bool,
        preview: bool,
        limit: Option<usize>,
        insert: bool,
    ) -> HxResult<()> {
        self.paste_from_clipboard(raw, preview, limit, insert)
    }

    fn execute_inspector_command(&mut self) {
        let from_inspector = self
            .command_return_mode
            .is_some_and(|mode| mode.is_inspector());
        if !self.show_inspector {
            self.show_inspector = true;
            self.refresh_inspector();
            self.focus_inspector_or_warn();
        } else if !from_inspector {
            self.focus_inspector_or_warn();
        } else {
            self.mode = Mode::Normal;
            self.show_inspector = false;
            self.inspector = None;
            self.inspector_error = None;
        }
    }

    fn execute_format_command(&mut self, name: Option<String>) {
        self.show_inspector = true;
        match name {
            Some(name) => self.execute_named_format_command(name),
            None => {
                self.inspector_format_override = None;
                self.refresh_inspector();
                if self.focus_inspector_or_warn() {
                    self.set_info_status("format: auto");
                }
            }
        }
    }

    fn execute_named_format_command(&mut self, name: String) {
        if crate::format::detect::detect_by_name(&name, &mut self.document).is_some() {
            self.inspector_format_override = Some(name.to_lowercase());
            self.refresh_inspector();
            if self.focus_inspector_or_warn() {
                self.set_info_status(format!("format: {}", name));
            }
        } else {
            self.set_error_status(format!("unknown or mismatched format: {}", name));
        }
    }

    fn execute_search_command(
        &mut self,
        kind: SearchKind,
        pattern: Vec<u8>,
        backward: bool,
    ) -> HxResult<()> {
        let search = SearchState { kind, pattern };
        self.last_search = Some(search.clone());
        self.run_search(&search, search_direction(backward))
    }
}

fn search_direction(backward: bool) -> SearchDirection {
    if backward {
        SearchDirection::Backward
    } else {
        SearchDirection::Forward
    }
}
