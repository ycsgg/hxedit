use crate::app::{App, SearchKind, SearchState};
use crate::commands::parser::parse_command;
use crate::commands::types::Command;
use crate::error::{HxError, HxResult};
use crate::mode::Mode;

impl App {
    pub(crate) fn submit_command(&mut self) -> HxResult<()> {
        let return_mode = self.command_return_mode.unwrap_or(Mode::Normal);
        let command = parse_command(&self.command_buffer)?;
        self.command_buffer.clear();
        self.execute_command(command)?;
        self.mode = return_mode;
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
                self.status_message = format!("wrote {} [{}]", saved.display(), profile);
                Ok(())
            }
            Command::WriteQuit { path } => {
                let (saved, profile) = self.document.save(path)?;
                self.undo_stack.clear();
                self.cursor = self.clamp_offset(self.cursor);
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
            } => self.paste_from_clipboard(raw, preview, limit),
            Command::Copy { format, display } => self.copy_selection(format, display),
            Command::SearchAscii { pattern } => {
                let search = SearchState {
                    kind: SearchKind::Ascii,
                    pattern,
                };
                self.last_search = Some(search.clone());
                self.run_search(&search, crate::app::SearchDirection::Forward)
            }
            Command::SearchHex { pattern } => {
                let search = SearchState {
                    kind: SearchKind::Hex,
                    pattern,
                };
                self.last_search = Some(search.clone());
                self.run_search(&search, crate::app::SearchDirection::Forward)
            }
        }
    }
}
