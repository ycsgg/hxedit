#[cfg(feature = "symbols")]
use crate::app::SymbolState;
use crate::app::{App, SearchDirection, SearchKind, SearchState, SidePanelKind};
use crate::commands::parser::parse_command;
#[cfg(feature = "memory")]
use crate::commands::types::MemoryCommand;
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
            Command::Source { path } => self.execute_source_command(path),
            Command::Script { path } => self.execute_script_command(path),
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
                force,
            } => self.execute_replace_command(&needle, &replacement, allow_resize, force),
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
                self.execute_search_command(SearchKind::Ascii, pattern, backward, false)
            }
            Command::SearchHex {
                pattern,
                backward,
                deprecated_alias,
            } => self.execute_search_command(SearchKind::Hex, pattern, backward, deprecated_alias),
            #[cfg(feature = "disasm")]
            Command::SearchInstruction { pattern, backward } => {
                self.execute_instruction_search_command(pattern, backward)
            }
            #[cfg(feature = "symbols")]
            Command::SearchSymbol { pattern, backward } => {
                self.execute_symbol_search_command(pattern, backward)
            }
            #[cfg(feature = "memory")]
            Command::MemorySearch { query, backward } => {
                self.execute_memory_search_command(query, backward)
            }
            Command::Hash { algorithm } => self.execute_hash_command(algorithm),
            Command::Diff(diff) => self.execute_diff_command(diff),
            Command::Stats(command) => {
                self.close_diff_projection_for_side_panel_switch();
                self.execute_stats_command(command)
            }
            #[cfg(feature = "sagitta-analysis")]
            Command::Analysis(command) => self.execute_analysis_command(command),
            #[cfg(feature = "memory")]
            Command::Memory(command) => self.execute_memory_command(command),
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

#[cfg(feature = "memory")]
impl App {
    fn execute_memory_command(&mut self, command: MemoryCommand) -> HxResult<()> {
        let message = match command {
            MemoryCommand::Open => self.memory_runtime().map_or_else(
                || "memory panel opened; no memory session is active".to_owned(),
                |runtime| {
                    let process = runtime.session.process_info();
                    format!("memory panel opened for {} ({})", process.name, process.pid)
                },
            ),
            MemoryCommand::List => {
                match crate::memory::list_processes() {
                    Ok(processes) => {
                        let message = format!("{} processes (Enter to attach)", processes.len());
                        self.open_memory_process_list_panel(processes, message);
                    }
                    Err(err) => {
                        self.open_memory_panel(format!("memory process list unavailable: {err}"));
                    }
                }
                return Ok(());
            }
            MemoryCommand::Refresh => {
                let Some(runtime) = self.memory_runtime_mut() else {
                    self.open_memory_panel("memory maps refresh requires an active memory session");
                    return Ok(());
                };
                runtime.session.refresh_regions()?;
                let count = runtime.session.regions().count();
                if count == 0 {
                    runtime.selected_region = 0;
                    runtime.opened_region = 0;
                } else {
                    runtime.selected_region = runtime.selected_region.min(count - 1);
                    runtime.opened_region = runtime.opened_region.min(count - 1);
                    runtime.base_va = runtime
                        .session
                        .region(runtime.opened_region)
                        .map_or(runtime.base_va, |region| region.start);
                }
                format!("refreshed {count} memory regions")
            }
            MemoryCommand::Info => {
                let text = self.memory_info_text();
                self.open_memory_info_panel(text);
                return Ok(());
            }
            MemoryCommand::Freeze => return self.execute_memory_freeze_command(),
            MemoryCommand::Thaw => return self.execute_memory_thaw_command(),
            MemoryCommand::Commit => return self.commit_memory_document(false),
            MemoryCommand::CommitAll => return self.commit_memory_document(true),
        };
        self.open_memory_panel(message);
        Ok(())
    }
}

#[cfg(feature = "sagitta-analysis")]
mod analysis;
mod automation;
mod file_nav;
mod hash_diff;
mod inspector;
mod search_disasm;
#[cfg(feature = "symbols")]
mod symbols;
mod transform;
