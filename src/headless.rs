use anyhow::Result;

use crate::automation::MacroProgram;
use crate::cli::{Cli, CliTarget};
use crate::commands::parser::parse_command;
use crate::commands::types::{Command, ExportFormat, GotoTarget};
use crate::core::document::Document;
use crate::error::{HxError, HxResult};
use crate::exec::{
    execute_batch, ExecBatchOptions, ExecBatchReport, ExecCommand, ExecOffset, ExecRange,
    ExecScope, ExecSearchDirection, ExecSelection, ExecState, RangeSpace, SearchSelect,
};
use crate::util::parse::parse_offset;

pub fn run(cli: Cli) -> Result<()> {
    let config = cli.config()?;
    let target = cli.target()?;
    let path = match target {
        CliTarget::File(path) => path,
        CliTarget::Pid(_) | CliTarget::Process(_) => {
            return Err(HxError::InvalidCliSource(
                "--run and --command require a file target".to_owned(),
            )
            .into())
        }
    };

    let mut document = Document::open(&path, &config)?;
    let selection = parse_initial_selection(cli.select.as_deref(), &document)?;
    let cursor = if document.is_empty() {
        0
    } else {
        config.initial_offset.min(document.len() - 1)
    };
    let mut state = ExecState::new(cursor, selection);

    for path in &cli.run {
        execute_macro(path, &mut document, &mut state)?;
    }

    for command in &cli.command {
        execute_command(command, &mut document, &mut state)?;
    }

    Ok(())
}

fn parse_initial_selection(
    input: Option<&str>,
    document: &Document,
) -> HxResult<Option<ExecSelection>> {
    let Some(input) = input else {
        return Ok(None);
    };

    let mut parts = input.split(':');
    let space = parts
        .next()
        .ok_or(HxError::MissingArgument("selection space"))?;
    let start = parts
        .next()
        .ok_or(HxError::MissingArgument("selection start"))?;
    let len = parts
        .next()
        .ok_or(HxError::MissingArgument("selection length"))?;
    if parts.next().is_some() {
        return Err(HxError::CommandError(format!(
            "invalid selection {input}; expected display:<start>:<len> or logical:<start>:<len>"
        )));
    }

    let space = match space {
        "display" => RangeSpace::Display,
        "logical" => RangeSpace::Logical,
        other => {
            return Err(HxError::CommandError(format!(
                "invalid selection space {other}; expected display or logical"
            )))
        }
    };
    let start = parse_offset(start)?;
    let len = parse_offset(len)?;
    if len == 0 {
        return Err(HxError::CommandError(
            "selection length must be greater than 0".to_owned(),
        ));
    }

    let range = ExecRange { space, start, len };
    range.display_bounds(document)?;
    Ok(Some(ExecSelection { range }))
}

fn execute_macro(
    path: &std::path::Path,
    document: &mut Document,
    state: &mut ExecState,
) -> HxResult<()> {
    let program = MacroProgram::load(path)?;
    let report = program.execute(document, state)?;
    let label = format!("source {}", path.display());
    finish_report(&label, report)
}

fn execute_command(input: &str, document: &mut Document, state: &mut ExecState) -> HxResult<()> {
    let command = parse_command(input)?;
    match map_command(command, state)? {
        HeadlessAction::Exec(command) => {
            let report = execute_batch(document, state, &[command], ExecBatchOptions::default())?;
            finish_report(input, report)
        }
        HeadlessAction::Source(path) => execute_macro(&path, document, state),
    }
}

enum HeadlessAction {
    Exec(ExecCommand),
    Source(std::path::PathBuf),
}

fn map_command(command: Command, state: &ExecState) -> HxResult<HeadlessAction> {
    let action = match command {
        Command::Write { path } | Command::WriteQuit { path } => {
            HeadlessAction::Exec(ExecCommand::Save { path })
        }
        Command::Source { path } => HeadlessAction::Source(path),
        Command::Fill { pattern, len } => HeadlessAction::Exec(ExecCommand::Fill {
            offset: ExecOffset::Cursor(0),
            pattern,
            len: len as u64,
        }),
        Command::Goto { target } => HeadlessAction::Exec(ExecCommand::Goto {
            target: map_goto_target(target),
        }),
        Command::Export {
            format: ExportFormat::Binary { path },
        } => HeadlessAction::Exec(ExecCommand::ExportBinary {
            scope: active_scope_or_all(state),
            path,
        }),
        Command::Xor {
            key,
            in_place: true,
        } => HeadlessAction::Exec(ExecCommand::XorInPlace {
            scope: selection_scope(state)?,
            key,
        }),
        Command::Replace {
            needle,
            replacement,
            allow_resize,
            force,
        } => HeadlessAction::Exec(ExecCommand::Replace {
            scope: active_scope_or_all(state),
            needle,
            replacement,
            allow_resize,
            force,
        }),
        Command::SearchAscii { pattern, backward }
        | Command::SearchHex {
            pattern, backward, ..
        } => HeadlessAction::Exec(ExecCommand::Search {
            pattern,
            direction: if backward {
                ExecSearchDirection::Backward
            } else {
                ExecSearchDirection::Forward
            },
            select: SearchSelect::None,
        }),
        Command::Hash { algorithm } => HeadlessAction::Exec(ExecCommand::Hash {
            algorithm,
            scope: active_scope_or_all(state),
        }),
        Command::Quit { .. } => return unsupported("q"),
        Command::Paste { .. } | Command::PasteInsert { .. } => return unsupported("paste"),
        Command::Undo { .. } => return unsupported("undo"),
        Command::Redo { .. } => return unsupported("redo"),
        Command::Copy { .. } => return unsupported("copy"),
        Command::Export { .. } => return unsupported("export text formats"),
        Command::Xor {
            in_place: false, ..
        } => return unsupported("xor"),
        #[cfg(feature = "disasm")]
        Command::SearchInstruction { .. } => return unsupported("search-instruction"),
        #[cfg(feature = "symbols")]
        Command::SearchSymbol { .. } => return unsupported("search-symbol"),
        #[cfg(feature = "memory")]
        Command::MemorySearch { .. } => return unsupported("memory search"),
        Command::Inspector | Command::InspectorMore => return unsupported("inspector"),
        Command::Format { .. } => return unsupported("format"),
        Command::Diff(_) => return unsupported("diff"),
        #[cfg(feature = "sagitta-analysis")]
        Command::Analysis(_) => return unsupported("analysis"),
        #[cfg(feature = "memory")]
        Command::Memory(_) => return unsupported("memory"),
        #[cfg(feature = "disasm")]
        Command::Disassemble { .. }
        | Command::DisassembleForce { .. }
        | Command::DisassembleOff => return unsupported("disassemble"),
        #[cfg(feature = "symbols")]
        Command::Symbols | Command::SymbolsOff => return unsupported("symbols"),
        Command::Data | Command::DataOff => return unsupported("data"),
    };
    Ok(action)
}

fn map_goto_target(target: GotoTarget) -> ExecOffset {
    match target {
        GotoTarget::Absolute(offset) => ExecOffset::Absolute(offset),
        GotoTarget::Relative(delta) => ExecOffset::Cursor(delta),
        GotoTarget::End => ExecOffset::End,
    }
}

fn active_scope_or_all(state: &ExecState) -> ExecScope {
    if state.selection.is_some() {
        ExecScope::Selection
    } else {
        ExecScope::All
    }
}

fn selection_scope(state: &ExecState) -> HxResult<ExecScope> {
    if state.selection.is_some() {
        Ok(ExecScope::Selection)
    } else {
        Err(HxError::MissingSelection)
    }
}

fn unsupported(name: &str) -> HxResult<HeadlessAction> {
    Err(HxError::CommandError(format!(
        "headless --command does not support :{name}"
    )))
}

fn finish_report(label: &str, report: ExecBatchReport) -> HxResult<()> {
    if report.outcomes.is_empty() {
        println!("{label}: no steps");
    } else {
        for outcome in &report.outcomes {
            println!("{label}: {}", outcome.summary);
            for warning in &outcome.warnings {
                println!("{label}: warning: {warning}");
            }
        }
    }

    if let Some(error) = report.error {
        return Err(HxError::CommandError(format!(
            "{label}: stopped after {} steps: {error}",
            report.steps_completed
        )));
    }

    Ok(())
}
