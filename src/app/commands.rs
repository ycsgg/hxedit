#[cfg(feature = "symbols")]
use crate::app::SymbolState;
use crate::app::{
    App, EditOp, ReplacementChange, SearchDirection, SearchKind, SearchState, SidePanelKind,
};
use crate::commands::parser::parse_command;
use crate::commands::types::{Command, DiffCommand, ExportFormat, GotoTarget, HashAlgorithm};
#[cfg(feature = "disasm")]
use crate::disasm::backend::resolve_backend_kind;
#[cfg(feature = "disasm")]
use crate::disasm::DisassemblyState;
use crate::error::{HxError, HxResult};
#[cfg(feature = "disasm")]
use crate::executable::{detect_executable_info, force_raw_executable_info, override_arch};
use crate::format::parse::StructValue;
use crate::mode::Mode;

impl App {
    pub(crate) fn submit_command(&mut self) -> HxResult<()> {
        let return_mode = self.command_return_mode.unwrap_or(Mode::Normal);
        let command = parse_command(&self.command_buffer)?;
        self.execute_command(command)?;
        self.remember_command_submission();
        self.command_buffer.clear();
        self.command_cursor_pos = 0;
        if matches!(self.mode, Mode::Command) {
            self.mode = self.normalize_mode(return_mode);
        }
        self.command_return_mode = None;
        self.reset_command_history_navigation();
        Ok(())
    }

    pub(crate) fn execute_command(&mut self, command: Command) -> HxResult<()> {
        match command {
            Command::Quit { force } => self.execute_quit_command(force),
            Command::Write { path } => self.execute_write_command(path, false),
            Command::WriteQuit { path } => self.execute_write_command(path, true),
            Command::Fill { pattern, len } => self.execute_fill_command(&pattern, len),
            Command::Goto { target } => self.execute_goto_command(target),
            Command::Undo { steps } => self.undo(steps, false),
            Command::Redo { steps } => self.redo(steps, false),
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
            Command::Export { format } => self.execute_export_command(format),
            Command::Xor { key, in_place } => self.execute_xor_command(key, in_place),
            Command::Replace {
                needle,
                replacement,
                allow_resize,
            } => self.execute_replace_command(&needle, &replacement, allow_resize),
            Command::Inspector => {
                self.close_diff_projection_for_side_panel_switch();
                self.execute_inspector_command();
                Ok(())
            }
            Command::InspectorMore => {
                self.execute_inspector_more_command();
                Ok(())
            }
            Command::Format { name } => {
                self.close_diff_projection_for_side_panel_switch();
                self.execute_format_command(name);
                Ok(())
            }
            Command::SearchAscii { pattern, backward } => {
                self.execute_search_command(SearchKind::Ascii, pattern, backward)
            }
            Command::SearchHex { pattern, backward } => {
                self.execute_search_command(SearchKind::Hex, pattern, backward)
            }
            #[cfg(feature = "disasm")]
            Command::SearchInstruction { pattern, backward } => {
                self.execute_instruction_search_command(pattern, backward)
            }
            #[cfg(feature = "symbols")]
            Command::SearchSymbol { pattern, backward } => {
                self.execute_symbol_search_command(pattern, backward)
            }
            Command::Hash { algorithm } => self.execute_hash_command(algorithm),
            Command::Diff(diff) => self.execute_diff_command(diff),
            #[cfg(feature = "disasm")]
            Command::Disassemble { arch } => self.execute_disassemble_command(arch.as_deref()),
            #[cfg(feature = "disasm")]
            Command::DisassembleForce { arch, offset } => {
                self.execute_disassemble_force_command(&arch, offset)
            }
            #[cfg(feature = "disasm")]
            Command::DisassembleOff => {
                self.execute_disassemble_off_command();
                Ok(())
            }
            #[cfg(feature = "symbols")]
            Command::Symbols => {
                self.close_diff_projection_for_side_panel_switch();
                self.execute_symbols_command()
            }
            #[cfg(feature = "symbols")]
            Command::SymbolsOff => {
                self.execute_symbols_off_command();
                Ok(())
            }
            Command::Data => {
                self.close_diff_projection_for_side_panel_switch();
                self.open_data_panel();
                Ok(())
            }
            Command::DataOff => {
                self.close_data_panel();
                Ok(())
            }
        }
    }
}

mod file_nav;
mod hash_diff;
mod inspector;
mod search_disasm;
#[cfg(feature = "symbols")]
mod symbols;
mod transform;
