use crate::app::{App, SearchDirection, SearchKind, SearchState};
use crate::commands::parser::parse_command;
use crate::commands::types::Command;
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

impl App {
    pub(crate) fn submit_command(&mut self) -> HxResult<()> {
        let return_mode = self.command_return_mode.unwrap_or(Mode::Normal);
        let command = parse_command(&self.command_buffer)?;
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        self.execute_command(command)?;
        if matches!(self.mode, Mode::Command) {
            self.mode = self.normalize_mode(return_mode);
        }
        self.command_return_mode = None;
        Ok(())
    }

    pub(crate) fn execute_command(&mut self, command: Command) -> HxResult<()> {
        match command {
            Command::Quit { force } => {
                if self.document.is_dirty() && !force {
                    return Err(HxError::DirtyQuit);
                }
                self.should_quit = true;
                Ok(())
            }
            Command::Write { path } => {
                let (saved, profile) = self.document.save(path)?;
                self.undo_stack.clear();
                self.cursor = self.clamp_offset(self.cursor);
                self.refresh_inspector();
                self.status_message = format!("wrote {} [{}]", saved.display(), profile);
                Ok(())
            }
            Command::WriteQuit { path } => {
                let (saved, profile) = self.document.save(path)?;
                self.undo_stack.clear();
                self.cursor = self.clamp_offset(self.cursor);
                self.refresh_inspector();
                self.status_message = format!("wrote {} [{}]", saved.display(), profile);
                self.should_quit = true;
                Ok(())
            }
            Command::Goto { offset } => {
                self.cursor = self.document.goto(offset)?;
                self.status_message = format!("goto 0x{:x}", self.cursor);
                Ok(())
            }
            Command::Undo { steps } => self.undo(steps, false),
            Command::Paste {
                raw,
                preview,
                limit,
            } => self.paste_from_clipboard(raw, preview, limit, false),
            Command::PasteInsert {
                raw,
                preview,
                limit,
            } => self.paste_from_clipboard(raw, preview, limit, true),
            Command::Copy { format, display } => self.copy_selection(format, display),
            Command::Inspector => {
                let from_inspector = self
                    .command_return_mode
                    .is_some_and(|mode| mode.is_inspector());
                if !self.show_inspector {
                    self.show_inspector = true;
                    self.refresh_inspector();
                    self.mode = Mode::Inspector;
                } else if !from_inspector {
                    self.mode = Mode::Inspector;
                    self.sync_inspector_to_cursor();
                } else {
                    self.mode = Mode::Normal;
                    self.show_inspector = false;
                    self.inspector = None;
                }
                Ok(())
            }
            Command::Format { name } => {
                self.show_inspector = true;
                if let Some(name) = name {
                    if crate::format::detect::detect_by_name(&name, &mut self.document).is_some() {
                        self.inspector_format_override = Some(name.to_lowercase());
                        self.refresh_inspector();
                        self.mode = Mode::Inspector;
                        self.status_message = format!("format: {}", name);
                    } else {
                        self.status_message = format!("unknown or mismatched format: {}", name);
                    }
                } else {
                    self.inspector_format_override = None;
                    self.refresh_inspector();
                    self.mode = Mode::Inspector;
                    self.status_message = "format: auto".to_owned();
                }
                Ok(())
            }
            Command::SearchAscii { pattern, backward } => {
                let search = SearchState {
                    kind: SearchKind::Ascii,
                    pattern,
                };
                self.last_search = Some(search.clone());
                let direction = if backward {
                    SearchDirection::Backward
                } else {
                    SearchDirection::Forward
                };
                self.run_search(&search, direction)
            }
            Command::SearchHex { pattern, backward } => {
                let search = SearchState {
                    kind: SearchKind::Hex,
                    pattern,
                };
                self.last_search = Some(search.clone());
                let direction = if backward {
                    SearchDirection::Backward
                } else {
                    SearchDirection::Forward
                };
                self.run_search(&search, direction)
            }
        }
    }
}
