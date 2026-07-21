use std::path::PathBuf;

#[cfg(feature = "sagitta-analysis")]
use super::ANALYSIS_ALIASES;
use super::{
    is_alias, BOOKMARK_ALIASES, COPY_ALIASES, DATA_ALIASES, DIFF_ALIASES, EXPORT_ALIASES,
    FILL_ALIASES, FORMAT_ALIASES, GOTO_ALIASES, HASH_ALIASES, INSPECTOR_ALIASES,
    LEGACY_HEX_SEARCH_ALIASES, PASTE_ALIASES, PASTE_INSERT_ALIASES, QUIT_ALIASES,
    QUIT_FORCE_ALIASES, REDO_ALIASES, REPLACE_ALIASES, SCRIPT_ALIASES, SEARCH_ALIASES,
    SOURCE_ALIASES, STATS_ALIASES, UNDO_ALIASES, WRITE_ALIASES, WRITE_QUIT_ALIASES, XOR_ALIASES,
    ZERO_ALIASES,
};
#[cfg(feature = "disasm")]
use super::{DISASSEMBLE_ALIASES, DISASSEMBLE_FORCE_ALIASES, INSTRUCTION_SEARCH_ALIASES};
#[cfg(feature = "memory")]
use super::{MEMORY_ALIASES, MEMORY_SEARCH_ALIASES};
#[cfg(feature = "symbols")]
use super::{SYMBOL_PANEL_ALIASES, SYMBOL_SEARCH_ALIASES};
use crate::commands::{
    split_command,
    types::{
        BookmarkColorArg, BookmarkCommand, Command, DiffCommand, ExportFormat, GotoTarget,
        HashAlgorithm, StatsCommand,
    },
};
use crate::copy::{CopyDisplay, CopyFormat};
use crate::error::{HxError, HxResult};
use crate::util::parse::{parse_hex_bytes, parse_hex_stream, parse_offset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaceInputMode {
    Hex,
    Ascii,
}

/// Parse command-line mode input into an executable command.
pub fn parse_command(input: &str) -> HxResult<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(HxError::UnknownCommand(trimmed.to_owned()));
    }

    let (name, rest) = split_command(trimmed);
    match name {
        name if is_alias(name, QUIT_ALIASES) => Ok(Command::Quit { force: false }),
        name if is_alias(name, QUIT_FORCE_ALIASES) => Ok(Command::Quit { force: true }),
        name if is_alias(name, WRITE_ALIASES) => Ok(Command::Write {
            path: opt_path(rest),
        }),
        name if is_alias(name, WRITE_QUIT_ALIASES) => Ok(Command::WriteQuit {
            path: opt_path(rest),
        }),
        name if is_alias(name, FILL_ALIASES) => parse_fill(rest),
        name if is_alias(name, ZERO_ALIASES) => parse_zero(rest),
        name if is_alias(name, REPLACE_ALIASES) => parse_replace(name, rest),
        name if is_alias(name, PASTE_ALIASES) => parse_paste(name, rest, false),
        name if is_alias(name, PASTE_INSERT_ALIASES) => parse_paste(name, rest, true),
        name if is_alias(name, COPY_ALIASES) => parse_copy(rest),
        name if is_alias(name, EXPORT_ALIASES) => parse_export(rest),
        name if is_alias(name, XOR_ALIASES) => parse_xor(name, rest),
        name if is_alias(name, UNDO_ALIASES) => Ok(Command::Undo {
            steps: parse_undo_steps(rest)?,
        }),
        name if is_alias(name, REDO_ALIASES) => Ok(Command::Redo {
            steps: parse_redo_steps(rest)?,
        }),
        name if is_alias(name, SOURCE_ALIASES) => {
            let path = rest
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .ok_or(HxError::MissingArgument("macro path"))?;
            Ok(Command::Source {
                path: PathBuf::from(path),
            })
        }
        name if is_alias(name, SCRIPT_ALIASES) => {
            let path = rest
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .ok_or(HxError::MissingArgument("script path"))?;
            Ok(Command::Script {
                path: PathBuf::from(path),
            })
        }
        name if is_alias(name, INSPECTOR_ALIASES) => match rest.map(str::trim) {
            None | Some("") => Ok(Command::Inspector),
            Some("more") => Ok(Command::InspectorMore),
            Some(other) => Err(HxError::UnknownCommand(format!("insp {other}"))),
        },
        name if is_alias(name, FORMAT_ALIASES) => Ok(Command::Format {
            name: rest.filter(|value| !value.is_empty()).map(str::to_owned),
        }),
        name if is_alias(name, GOTO_ALIASES) => {
            let arg = rest.ok_or(HxError::MissingArgument("offset"))?;
            Ok(Command::Goto {
                target: parse_goto_target(arg)?,
            })
        }
        name if is_alias(name, SEARCH_ALIASES) => parse_search(rest, name.ends_with('!')),
        name if is_alias(name, LEGACY_HEX_SEARCH_ALIASES) => {
            let arg = rest.ok_or(HxError::MissingArgument("hex search pattern"))?;
            Ok(Command::SearchHex {
                pattern: parse_hex_bytes(arg)?,
                backward: name.ends_with('!'),
                deprecated_alias: true,
            })
        }
        #[cfg(feature = "disasm")]
        name if is_alias(name, INSTRUCTION_SEARCH_ALIASES) => {
            let arg = rest.ok_or(HxError::MissingArgument("instruction search pattern"))?;
            if arg.is_empty() {
                return Err(HxError::EmptySearch);
            }
            Ok(Command::SearchInstruction {
                pattern: arg.to_owned(),
                backward: name.ends_with('!'),
            })
        }
        #[cfg(feature = "symbols")]
        name if is_alias(name, SYMBOL_SEARCH_ALIASES) => {
            let arg = rest.ok_or(HxError::MissingArgument("symbol search pattern"))?;
            if arg.is_empty() {
                return Err(HxError::EmptySearch);
            }
            Ok(Command::SearchSymbol {
                pattern: arg.to_owned(),
                backward: name.ends_with('!'),
            })
        }
        #[cfg(feature = "memory")]
        name if is_alias(name, MEMORY_SEARCH_ALIASES) => {
            let arg = rest.ok_or(HxError::MissingArgument("memory search pattern"))?;
            Ok(Command::MemorySearch {
                query: crate::memory::MemorySearchQuery::parse(arg)?,
                backward: name.ends_with('!'),
            })
        }
        name if is_alias(name, HASH_ALIASES) => {
            let arg = rest.ok_or(HxError::MissingArgument("hash algorithm"))?;
            let algo = HashAlgorithm::parse(arg)
                .ok_or_else(|| HxError::InvalidHashAlgorithm(arg.to_owned()))?;
            Ok(Command::Hash { algorithm: algo })
        }
        name if is_alias(name, STATS_ALIASES) => parse_stats(rest),
        name if is_alias(name, BOOKMARK_ALIASES) => parse_bookmark(name, rest),
        name if is_alias(name, DIFF_ALIASES) => parse_diff(rest),
        #[cfg(feature = "sagitta-analysis")]
        name if is_alias(name, ANALYSIS_ALIASES) => parse_analysis(rest),
        #[cfg(feature = "memory")]
        name if is_alias(name, MEMORY_ALIASES) => parse_memory(rest),
        #[cfg(feature = "disasm")]
        name if is_alias(name, DISASSEMBLE_ALIASES) => match rest.map(str::trim) {
            None | Some("") => Ok(Command::Disassemble { arch: None }),
            Some("off") => Ok(Command::DisassembleOff),
            Some(arg) => Ok(Command::Disassemble {
                arch: Some(arg.to_owned()),
            }),
        },
        #[cfg(feature = "disasm")]
        name if is_alias(name, DISASSEMBLE_FORCE_ALIASES) => {
            let rest = rest.ok_or(HxError::MissingArgument("arch offset"))?;
            let mut parts = rest.split_whitespace();
            let arch = parts.next().ok_or(HxError::MissingArgument("arch"))?;
            let offset = parts.next().ok_or(HxError::MissingArgument("offset"))?;
            if parts.next().is_some() {
                return Err(HxError::UnknownCommand(rest.to_owned()));
            }
            Ok(Command::DisassembleForce {
                arch: arch.to_owned(),
                offset: parse_offset(offset)?,
            })
        }
        #[cfg(feature = "symbols")]
        name if is_alias(name, SYMBOL_PANEL_ALIASES) => match rest.map(str::trim) {
            None | Some("") => Ok(Command::Symbols),
            Some("off") => Ok(Command::SymbolsOff),
            Some(other) => Err(HxError::UnknownCommand(format!("sym {other}"))),
        },
        name if is_alias(name, DATA_ALIASES) => match rest.map(str::trim) {
            None | Some("") => Ok(Command::Data),
            Some("off") => Ok(Command::DataOff),
            Some(other) => Err(HxError::UnknownCommand(format!("data {other}"))),
        },
        other => Err(HxError::UnknownCommand(other.to_owned())),
    }
}

fn parse_bookmark(name: &str, rest: Option<&str>) -> HxResult<Command> {
    if name == "marks" {
        if rest.is_some_and(|rest| !rest.trim().is_empty()) {
            return Err(HxError::CommandError(
                ":marks does not accept arguments".to_owned(),
            ));
        }
        return Ok(Command::Bookmark(BookmarkCommand::Panel));
    }
    let Some(rest) = rest.map(str::trim).filter(|rest| !rest.is_empty()) else {
        return Ok(Command::Bookmark(BookmarkCommand::Panel));
    };
    let (subcommand, tail) = split_command(rest);
    let command = match subcommand {
        "add" => parse_bookmark_add(tail)?,
        "note" => {
            let tail = tail.ok_or(HxError::MissingArgument("bookmark selector"))?;
            let (selector, note) = split_command(tail);
            BookmarkCommand::Note {
                selector: selector.to_owned(),
                note: note
                    .map(str::trim)
                    .filter(|note| !note.is_empty())
                    .map(str::to_owned),
            }
        }
        "del" => BookmarkCommand::Delete {
            selector: parse_bookmark_selector(tail)?,
        },
        "clear" => {
            reject_bookmark_tail("clear", tail)?;
            BookmarkCommand::Clear
        }
        "goto" => BookmarkCommand::Goto {
            selector: parse_bookmark_selector(tail)?,
        },
        "next" => {
            reject_bookmark_tail("next", tail)?;
            BookmarkCommand::Next
        }
        "prev" => {
            reject_bookmark_tail("prev", tail)?;
            BookmarkCommand::Prev
        }
        other => return Err(HxError::UnknownCommand(format!("mark {other}"))),
    };
    Ok(Command::Bookmark(command))
}

fn parse_bookmark_add(rest: Option<&str>) -> HxResult<BookmarkCommand> {
    let parts = rest
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>();
    let mut name = None;
    let mut start = None;
    let mut len = None;
    let mut color = BookmarkColorArg::Default;
    let mut has_color = false;
    let mut note = None;
    let mut index = 0;
    while index < parts.len() {
        match parts[index] {
            "--at" => {
                if start.is_some() {
                    return Err(HxError::CommandError(
                        "duplicate bookmark option: --at".to_owned(),
                    ));
                }
                let value = parts
                    .get(index + 1)
                    .ok_or(HxError::MissingArgument("bookmark offset"))?;
                start = Some(parse_offset(value)?);
                index += 2;
            }
            "--len" => {
                if len.is_some() {
                    return Err(HxError::CommandError(
                        "duplicate bookmark option: --len".to_owned(),
                    ));
                }
                let value = parts
                    .get(index + 1)
                    .ok_or(HxError::MissingArgument("bookmark length"))?;
                len = Some(parse_offset(value)?);
                index += 2;
            }
            "--color" => {
                if has_color {
                    return Err(HxError::CommandError(
                        "duplicate bookmark option: --color".to_owned(),
                    ));
                }
                let value = parts
                    .get(index + 1)
                    .ok_or(HxError::MissingArgument("bookmark color"))?;
                color = BookmarkColorArg::parse(value).ok_or_else(|| {
                    HxError::CommandError(format!("invalid bookmark color: {value}"))
                })?;
                has_color = true;
                index += 2;
            }
            "--note" => {
                let value = parts[index + 1..].join(" ");
                if value.is_empty() {
                    return Err(HxError::MissingArgument("bookmark note"));
                }
                note = Some(value);
                break;
            }
            option if option.starts_with("--") => {
                return Err(HxError::CommandError(format!(
                    "unknown bookmark option: {option}"
                )));
            }
            value => {
                if name.is_some() {
                    return Err(HxError::CommandError(format!(
                        "unexpected bookmark argument: {value}; use --note for comments"
                    )));
                }
                name = Some(value.to_owned());
                index += 1;
            }
        }
    }
    if len.is_some() && start.is_none() {
        return Err(HxError::CommandError(
            "bookmark --len requires --at".to_owned(),
        ));
    }
    Ok(BookmarkCommand::Add {
        name,
        start,
        len,
        color,
        note,
    })
}

fn parse_bookmark_selector(tail: Option<&str>) -> HxResult<String> {
    let tail = tail
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
        .ok_or(HxError::MissingArgument("bookmark selector"))?;
    let (selector, extra) = split_command(tail);
    if extra.is_some_and(|extra| !extra.is_empty()) {
        return Err(HxError::CommandError(
            "bookmark selector must be a name or #id".to_owned(),
        ));
    }
    Ok(selector.to_owned())
}

fn reject_bookmark_tail(subcommand: &str, tail: Option<&str>) -> HxResult<()> {
    if tail.is_some_and(|tail| !tail.trim().is_empty()) {
        return Err(HxError::CommandError(format!(
            "mark {subcommand} does not accept arguments"
        )));
    }
    Ok(())
}

fn parse_stats(rest: Option<&str>) -> HxResult<Command> {
    let command = match rest.map(str::trim) {
        None | Some("") => StatsCommand::Auto,
        Some("all") => StatsCommand::All,
        Some("selection") | Some("sel") => StatsCommand::Selection,
        Some("refresh") => StatsCommand::Refresh,
        Some("off") => StatsCommand::Off,
        Some(other) => return Err(HxError::UnknownCommand(format!("stats {other}"))),
    };
    Ok(Command::Stats(command))
}

#[cfg(feature = "sagitta-analysis")]
fn parse_analysis(rest: Option<&str>) -> HxResult<Command> {
    let command = match rest.map(str::trim) {
        None | Some("") => crate::commands::types::AnalysisCommand::Run,
        Some("status") => crate::commands::types::AnalysisCommand::Status,
        Some("off") => crate::commands::types::AnalysisCommand::Off,
        Some(other) => return Err(HxError::UnknownCommand(format!("ana {other}"))),
    };
    Ok(Command::Analysis(command))
}

#[cfg(feature = "memory")]
fn parse_memory(rest: Option<&str>) -> HxResult<Command> {
    use crate::commands::types::MemoryCommand;

    let command = match rest.map(str::trim) {
        None | Some("") => MemoryCommand::Open,
        Some("list") => MemoryCommand::List,
        Some("refresh") => MemoryCommand::Refresh,
        Some("info") => MemoryCommand::Info,
        Some("freeze") => MemoryCommand::Freeze,
        Some("thaw") => MemoryCommand::Thaw,
        Some("commit") => MemoryCommand::Commit,
        Some("commit-all") => MemoryCommand::CommitAll,
        Some(other) => return Err(HxError::UnknownCommand(format!("mem {other}"))),
    };
    Ok(Command::Memory(command))
}

fn parse_goto_target(input: &str) -> HxResult<GotoTarget> {
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("end") {
        return Ok(GotoTarget::End);
    }

    if let Some(relative) = trimmed.strip_prefix('+') {
        let offset = parse_offset(relative)?;
        let delta =
            i64::try_from(offset).map_err(|_| HxError::InvalidOffset(trimmed.to_owned()))?;
        return Ok(GotoTarget::Relative(delta));
    }

    if let Some(relative) = trimmed.strip_prefix('-') {
        let offset = parse_offset(relative)?;
        let delta =
            i64::try_from(offset).map_err(|_| HxError::InvalidOffset(trimmed.to_owned()))?;
        return Ok(GotoTarget::Relative(-delta));
    }

    Ok(GotoTarget::Absolute(parse_offset(trimmed)?))
}

fn parse_search(input: Option<&str>, backward: bool) -> HxResult<Command> {
    let arg = input
        .ok_or(HxError::MissingArgument("search pattern"))?
        .trim();
    if arg.is_empty() {
        return Err(HxError::EmptySearch);
    }

    let Some((mode, body, _rest)) = parse_search_delimited_pattern(arg)? else {
        // Backward-compatible plain text form. The documented command surface is
        // now `:s [mode]<delim><text><delim>`, but keeping `:s foo` avoids
        // breaking existing muscle memory while `:S` is the only deprecated
        // alias that gets an explicit warning.
        return Ok(Command::SearchAscii {
            pattern: arg.as_bytes().to_vec(),
            backward,
        });
    };

    let (pattern, is_text) = parse_search_pattern(mode, body)?;
    if pattern.is_empty() {
        return Err(HxError::EmptySearch);
    }
    if is_text {
        Ok(Command::SearchAscii { pattern, backward })
    } else {
        Ok(Command::SearchHex {
            pattern,
            backward,
            deprecated_alias: false,
        })
    }
}

fn parse_search_delimited_pattern(input: &str) -> HxResult<Option<(&str, &str, &str)>> {
    let Some((delim_idx, delimiter)) = input
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_ascii_alphanumeric()).then_some((idx, ch)))
    else {
        return Ok(None);
    };

    let mode = &input[..delim_idx];
    let body_start = delim_idx + delimiter.len_utf8();
    let body = &input[body_start..];
    let Some(end_rel) = body.find(delimiter) else {
        return Err(HxError::MissingArgument("search closing delimiter"));
    };
    let pattern = &body[..end_rel];
    let rest = body[end_rel + delimiter.len_utf8()..].trim();
    if !rest.is_empty() {
        return Err(HxError::UnknownCommand(format!("s {rest}")));
    }
    Ok(Some((mode, pattern, rest)))
}

fn parse_search_pattern(mode: &str, body: &str) -> HxResult<(Vec<u8>, bool)> {
    parse_mode_pattern("s", mode, body)
}

fn parse_mode_pattern(command: &str, mode: &str, body: &str) -> HxResult<(Vec<u8>, bool)> {
    match mode {
        "" | "s" | "str" | "utf8" => Ok((body.as_bytes().to_vec(), true)),
        "x" | "hex" => parse_hex_stream(body).map(|bytes| (bytes, false)),
        "b" | "byte" => {
            let value = parse_offset(body)?;
            let byte = u8::try_from(value).map_err(|_| HxError::InvalidOffset(body.to_owned()))?;
            Ok((vec![byte], false))
        }
        "u32" | "u32le" => parse_u32(body).map(|value| (value.to_le_bytes().to_vec(), false)),
        "u32be" => parse_u32(body).map(|value| (value.to_be_bytes().to_vec(), false)),
        "u64" | "u64le" => parse_offset(body).map(|value| (value.to_le_bytes().to_vec(), false)),
        "u64be" => parse_offset(body).map(|value| (value.to_be_bytes().to_vec(), false)),
        "i32" | "i32le" => parse_i32(body).map(|value| (value.to_le_bytes().to_vec(), false)),
        "i32be" => parse_i32(body).map(|value| (value.to_be_bytes().to_vec(), false)),
        "i64" | "i64le" => parse_i64(body).map(|value| (value.to_le_bytes().to_vec(), false)),
        "i64be" => parse_i64(body).map(|value| (value.to_be_bytes().to_vec(), false)),
        other => Err(HxError::UnknownCommand(format!("{command} {other}/.../"))),
    }
}

fn parse_u32(input: &str) -> HxResult<u32> {
    u32::try_from(parse_offset(input)?).map_err(|_| HxError::InvalidOffset(input.to_owned()))
}

fn parse_i32(input: &str) -> HxResult<i32> {
    i32::try_from(parse_i64(input)?).map_err(|_| HxError::InvalidOffset(input.to_owned()))
}

fn parse_i64(input: &str) -> HxResult<i64> {
    let trimmed = input.trim();
    if let Some(hex) = trimmed.strip_prefix("-0x") {
        let value =
            i64::from_str_radix(hex, 16).map_err(|_| HxError::InvalidOffset(input.to_owned()))?;
        Ok(-value)
    } else if let Some(hex) = trimmed.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).map_err(|_| HxError::InvalidOffset(input.to_owned()))
    } else {
        trimmed
            .parse::<i64>()
            .map_err(|_| HxError::InvalidOffset(input.to_owned()))
    }
}

fn opt_path(input: Option<&str>) -> Option<PathBuf> {
    input.filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn parse_diff(input: Option<&str>) -> HxResult<Command> {
    let rest = input
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
        .ok_or(HxError::MissingArgument("diff path|refresh|next|prev|off"))?;

    match rest {
        "refresh" => return Ok(Command::Diff(DiffCommand::Refresh)),
        "next" => return Ok(Command::Diff(DiffCommand::Next)),
        "prev" => return Ok(Command::Diff(DiffCommand::Prev)),
        "off" => return Ok(Command::Diff(DiffCommand::Off)),
        _ => {}
    }

    if let Some(after_flag) = rest.strip_prefix("-n") {
        let after_flag = after_flag.trim_start();
        let (raw_n, path) = split_command(after_flag);
        if raw_n.is_empty() {
            return Err(HxError::MissingArgument("diff max shift"));
        }
        let max_shift = usize::try_from(parse_offset(raw_n)?)
            .map_err(|_| HxError::InvalidOffset(raw_n.to_owned()))?;
        let path = path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .ok_or(HxError::MissingArgument("diff path"))?;
        return Ok(Command::Diff(DiffCommand::Open {
            path: PathBuf::from(path),
            max_shift: Some(max_shift),
        }));
    }

    Ok(Command::Diff(DiffCommand::Open {
        path: PathBuf::from(rest),
        max_shift: None,
    }))
}

fn parse_undo_steps(input: Option<&str>) -> HxResult<usize> {
    parse_positive_count(input, HxError::InvalidUndoCount)
}

fn parse_redo_steps(input: Option<&str>) -> HxResult<usize> {
    parse_positive_count(input, HxError::InvalidRedoCount)
}

fn parse_fill_count(input: &str) -> HxResult<usize> {
    parse_positive_usize(input, HxError::InvalidFillCount)
}

fn parse_positive_count(input: Option<&str>, invalid: fn(String) -> HxError) -> HxResult<usize> {
    match input {
        None => Ok(1),
        Some("") => Ok(1),
        Some(value) => parse_positive_usize(value, invalid),
    }
}

fn parse_positive_usize(input: &str, invalid: fn(String) -> HxError) -> HxResult<usize> {
    let steps = input
        .parse::<usize>()
        .map_err(|_| invalid(input.to_owned()))?;
    if steps == 0 {
        return Err(invalid(input.to_owned()));
    }
    Ok(steps)
}

fn parse_copy(input: Option<&str>) -> HxResult<Command> {
    let mut format = CopyFormat::Byte;
    let mut display = CopyDisplay::Raw;

    if let Some(rest) = input {
        for token in rest.split_whitespace() {
            if let Some(parsed) = CopyFormat::parse(token) {
                format = parsed;
                continue;
            }
            if let Some(parsed) = CopyDisplay::parse(token) {
                display = parsed;
                continue;
            }
            return Err(HxError::UnknownCommand(token.to_owned()));
        }
    }

    Ok(Command::Copy { format, display })
}

fn parse_paste(name: &str, input: Option<&str>, insert: bool) -> HxResult<Command> {
    let mut raw = name.contains('!');
    let preview = name.contains('?');
    let mut limit = None;

    if let Some(rest) = input {
        for token in rest.split_whitespace() {
            if token == "!" {
                raw = true;
                continue;
            }
            if limit.is_none() {
                let parsed = token
                    .parse::<usize>()
                    .map_err(|_| HxError::InvalidPasteCount(token.to_owned()))?;
                limit = Some(parsed);
                continue;
            }
            return Err(HxError::UnknownCommand(token.to_owned()));
        }
    }

    if insert {
        Ok(Command::PasteInsert {
            raw,
            preview,
            limit,
        })
    } else {
        Ok(Command::Paste {
            raw,
            preview,
            limit,
        })
    }
}

fn parse_fill(input: Option<&str>) -> HxResult<Command> {
    let rest = input.ok_or(HxError::MissingArgument("fill pattern and length"))?;
    let mut tokens = rest.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2 {
        return Err(HxError::MissingArgument("fill pattern and length"));
    }

    let len = parse_fill_count(tokens.pop().expect("fill len token"))?;
    let pattern = parse_hex_stream(&tokens.join(" "))?;
    Ok(Command::Fill { pattern, len })
}

fn parse_zero(input: Option<&str>) -> HxResult<Command> {
    let rest = input.ok_or(HxError::MissingArgument("fill length"))?;
    let len = parse_fill_count(rest)?;
    Ok(Command::Fill {
        pattern: vec![0],
        len,
    })
}

fn parse_export(input: Option<&str>) -> HxResult<Command> {
    let rest = input.ok_or(HxError::MissingArgument("export target"))?;
    let mut tokens = rest.split_whitespace();
    let first = tokens
        .next()
        .ok_or(HxError::MissingArgument("export target"))?;

    let format = match first {
        "bin" | "raw" => {
            let path = tokens.collect::<Vec<_>>().join(" ");
            if path.is_empty() {
                return Err(HxError::MissingArgument("export path"));
            }
            ExportFormat::Binary {
                path: PathBuf::from(path),
            }
        }
        "c" | "carray" | "c-array" => {
            let name = tokens.next().unwrap_or("");
            if let Some(extra) = tokens.next() {
                return Err(HxError::UnknownCommand(extra.to_owned()));
            }
            ExportFormat::CArray {
                name: name.to_owned(),
            }
        }
        "py" | "python" => {
            let name = tokens.next().unwrap_or("");
            if let Some(extra) = tokens.next() {
                return Err(HxError::UnknownCommand(extra.to_owned()));
            }
            ExportFormat::PythonBytes {
                name: name.to_owned(),
            }
        }
        _ => ExportFormat::Binary {
            path: PathBuf::from(rest),
        },
    };

    Ok(Command::Export { format })
}

fn parse_xor(name: &str, input: Option<&str>) -> HxResult<Command> {
    let raw = input
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
        .ok_or(HxError::MissingArgument("xor key"))?;
    let key = parse_xor_key(raw)?;
    Ok(Command::Xor {
        key,
        in_place: name.ends_with('!'),
    })
}

fn parse_xor_key(input: &str) -> HxResult<u8> {
    if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        if hex.is_empty() || hex.len() > 2 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(HxError::InvalidXorKey(input.to_owned()));
        }
        return u8::from_str_radix(hex, 16).map_err(|_| HxError::InvalidXorKey(input.to_owned()));
    }

    input
        .parse::<u8>()
        .map_err(|_| HxError::InvalidXorKey(input.to_owned()))
}

fn parse_replace(name: &str, input: Option<&str>) -> HxResult<Command> {
    let allow_resize = name.ends_with('!');
    let rest = input.ok_or(HxError::MissingArgument("replace arguments"))?;
    let (force, rest) = parse_replace_force(rest);
    let (needle, replacement) = if let Some((mode, needle_src, replacement_src)) =
        parse_replace_delimited(rest)?
    {
        (
            parse_replace_delimited_bytes(mode, needle_src)?,
            parse_replace_delimited_bytes(mode, replacement_src)?,
        )
    } else {
        let (mode, body) = parse_replace_mode(rest);
        let (needle_src, replacement_src) = body
            .split_once("->")
            .or_else(|| body.split_once("=>"))
            .ok_or_else(|| {
                HxError::InvalidReplace(
                    "expected [mode]/needle/replacement/ or <needle> -> <replacement>".to_owned(),
                )
            })?;

        (
            parse_replace_bytes(mode, needle_src.trim())?,
            parse_replace_bytes(mode, replacement_src.trim())?,
        )
    };

    if needle.is_empty() {
        return Err(HxError::InvalidReplace(
            "needle must not be empty".to_owned(),
        ));
    }
    if !allow_resize && needle.len() != replacement.len() {
        return Err(HxError::InvalidReplace(
            "equal-length replace requires same-size needle/replacement; use :re! to resize"
                .to_owned(),
        ));
    }

    Ok(Command::Replace {
        needle,
        replacement,
        allow_resize,
        force,
    })
}

fn parse_replace_force(input: &str) -> (bool, &str) {
    let trimmed = input.trim_start();
    let Some(rest) = trimmed.strip_prefix("--force") else {
        return (false, input);
    };
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        return (true, rest.trim_start());
    }
    (false, input)
}

fn parse_replace_delimited(input: &str) -> HxResult<Option<(&str, &str, &str)>> {
    let input = input.trim();
    let Some((delim_idx, delimiter)) = input
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_ascii_alphanumeric()).then_some((idx, ch)))
    else {
        return Ok(None);
    };
    if delimiter.is_whitespace() {
        return Ok(None);
    }

    let mode = &input[..delim_idx];
    let body_start = delim_idx + delimiter.len_utf8();
    let body = &input[body_start..];
    let Some(needle_end) = body.find(delimiter) else {
        return Err(HxError::MissingArgument("replace needle closing delimiter"));
    };
    let needle = &body[..needle_end];

    let replacement_start = needle_end + delimiter.len_utf8();
    let replacement_and_rest = &body[replacement_start..];
    let Some(replacement_end) = replacement_and_rest.find(delimiter) else {
        return Err(HxError::MissingArgument(
            "replace replacement closing delimiter",
        ));
    };
    let replacement = &replacement_and_rest[..replacement_end];
    let rest = replacement_and_rest[replacement_end + delimiter.len_utf8()..].trim();
    if !rest.is_empty() {
        return Err(HxError::UnknownCommand(format!("re {rest}")));
    }

    Ok(Some((mode, needle, replacement)))
}

fn parse_replace_mode(input: &str) -> (ReplaceInputMode, &str) {
    let trimmed = input.trim();
    for (prefix, mode) in [
        ("hex ", ReplaceInputMode::Hex),
        ("x ", ReplaceInputMode::Hex),
        ("ascii ", ReplaceInputMode::Ascii),
        ("text ", ReplaceInputMode::Ascii),
        ("a ", ReplaceInputMode::Ascii),
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return (mode, rest.trim());
        }
    }
    (ReplaceInputMode::Hex, trimmed)
}

fn parse_replace_bytes(mode: ReplaceInputMode, input: &str) -> HxResult<Vec<u8>> {
    match mode {
        ReplaceInputMode::Hex => parse_hex_stream(input),
        ReplaceInputMode::Ascii => Ok(strip_wrapping_quotes(input).as_bytes().to_vec()),
    }
}

fn parse_replace_delimited_bytes(mode: &str, input: &str) -> HxResult<Vec<u8>> {
    parse_mode_pattern("re", mode, input).map(|(bytes, _)| bytes)
}

fn strip_wrapping_quotes(input: &str) -> &str {
    input
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_aliases_parse() {
        assert_eq!(parse_command("q").unwrap(), Command::Quit { force: false });
        assert_eq!(
            parse_command("quit!").unwrap(),
            Command::Quit { force: true }
        );
    }

    #[test]
    fn inspector_aliases_parse() {
        assert_eq!(parse_command("insp").unwrap(), Command::Inspector);
        assert_eq!(parse_command("inspector").unwrap(), Command::Inspector);
    }

    #[test]
    fn data_command_parses() {
        assert_eq!(parse_command("data").unwrap(), Command::Data);
        assert_eq!(parse_command("data off").unwrap(), Command::DataOff);
    }

    #[test]
    fn bookmark_commands_parse() {
        assert_eq!(
            parse_command("marks").unwrap(),
            Command::Bookmark(BookmarkCommand::Panel)
        );
        assert_eq!(
            parse_command("mark add payload --at 0x100 --len 0x20").unwrap(),
            Command::Bookmark(BookmarkCommand::Add {
                name: Some("payload".to_owned()),
                start: Some(0x100),
                len: Some(0x20),
                color: BookmarkColorArg::Default,
                note: None,
            })
        );
        assert_eq!(
            parse_command("mark add --color green --at 0x10 --len 4 --note payload start").unwrap(),
            Command::Bookmark(BookmarkCommand::Add {
                name: None,
                start: Some(0x10),
                len: Some(4),
                color: BookmarkColorArg::Green,
                note: Some("payload start".to_owned()),
            })
        );
        assert_eq!(
            parse_command("mark add payload --note compressed stream").unwrap(),
            Command::Bookmark(BookmarkCommand::Add {
                name: Some("payload".to_owned()),
                start: None,
                len: None,
                color: BookmarkColorArg::Default,
                note: Some("compressed stream".to_owned()),
            })
        );
        assert_eq!(
            parse_command("mark note payload compressed stream").unwrap(),
            Command::Bookmark(BookmarkCommand::Note {
                selector: "payload".to_owned(),
                note: Some("compressed stream".to_owned()),
            })
        );
        assert_eq!(
            parse_command("mark goto #7").unwrap(),
            Command::Bookmark(BookmarkCommand::Goto {
                selector: "#7".to_owned(),
            })
        );
    }

    #[test]
    fn bookmark_parser_rejects_ambiguous_or_invalid_arguments() {
        for input in [
            "m add old-alias",
            "marks extra",
            "mark add payload 0x100",
            "mark add payload --at nope",
            "mark add payload --len 4",
            "mark add --color default --color red",
            "mark next extra",
            "mark goto one two",
        ] {
            assert!(parse_command(input).is_err(), "{input} should fail");
        }
    }

    #[cfg(feature = "memory")]
    #[test]
    fn memory_command_parses_subcommands() {
        use crate::commands::types::MemoryCommand;

        assert_eq!(
            parse_command("mem").unwrap(),
            Command::Memory(MemoryCommand::Open)
        );
        assert_eq!(
            parse_command("mem list").unwrap(),
            Command::Memory(MemoryCommand::List)
        );
        assert_eq!(
            parse_command("mem refresh").unwrap(),
            Command::Memory(MemoryCommand::Refresh)
        );
        assert_eq!(
            parse_command("mem info").unwrap(),
            Command::Memory(MemoryCommand::Info)
        );
        assert_eq!(
            parse_command("mem freeze").unwrap(),
            Command::Memory(MemoryCommand::Freeze)
        );
        assert_eq!(
            parse_command("mem thaw").unwrap(),
            Command::Memory(MemoryCommand::Thaw)
        );
        assert_eq!(
            parse_command("mem commit").unwrap(),
            Command::Memory(MemoryCommand::Commit)
        );
        assert_eq!(
            parse_command("mem commit-all").unwrap(),
            Command::Memory(MemoryCommand::CommitAll)
        );
        assert!(matches!(
            parse_command("mem unknown"),
            Err(HxError::UnknownCommand(name)) if name == "mem unknown"
        ));
    }

    #[cfg(feature = "memory")]
    #[test]
    fn memory_search_command_parses_pattern_and_filters() {
        let Command::MemorySearch { query, backward } =
            parse_command("ms x/4889c7/ in:r-x").unwrap()
        else {
            panic!("expected memory search command");
        };
        assert!(!backward);
        assert_eq!(query.pattern, vec![0x48, 0x89, 0xc7]);

        let Command::MemorySearch { query, backward } =
            parse_command("ms! /token/ in:rw-").unwrap()
        else {
            panic!("expected memory search command");
        };
        assert!(backward);
        assert_eq!(query.pattern, b"token");
    }

    #[test]
    fn unified_search_command_parses_modes() {
        assert_eq!(
            parse_command("s /hello/").unwrap(),
            Command::SearchAscii {
                pattern: b"hello".to_vec(),
                backward: false,
            }
        );
        assert_eq!(
            parse_command("s! @hello/world@").unwrap(),
            Command::SearchAscii {
                pattern: b"hello/world".to_vec(),
                backward: true,
            }
        );
        assert_eq!(
            parse_command("s x/48 89 c7/").unwrap(),
            Command::SearchHex {
                pattern: vec![0x48, 0x89, 0xc7],
                backward: false,
                deprecated_alias: false,
            }
        );
        assert_eq!(
            parse_command("s b/255/").unwrap(),
            Command::SearchHex {
                pattern: vec![0xff],
                backward: false,
                deprecated_alias: false,
            }
        );
        assert_eq!(
            parse_command("s u32be/0x12345678/").unwrap(),
            Command::SearchHex {
                pattern: vec![0x12, 0x34, 0x56, 0x78],
                backward: false,
                deprecated_alias: false,
            }
        );
        assert!(matches!(
            parse_command("s i16/1/"),
            Err(HxError::UnknownCommand(name)) if name == "s i16/.../"
        ));
    }

    #[test]
    fn legacy_hex_search_alias_is_marked_deprecated() {
        assert_eq!(
            parse_command("S! 7f 45 4c 46").unwrap(),
            Command::SearchHex {
                pattern: vec![0x7f, 0x45, 0x4c, 0x46],
                backward: true,
                deprecated_alias: true,
            }
        );
    }

    #[test]
    fn diff_command_parses_paths_and_subcommands() {
        assert_eq!(
            parse_command("diff other file.bin").unwrap(),
            Command::Diff(DiffCommand::Open {
                path: PathBuf::from("other file.bin"),
                max_shift: None,
            })
        );
        assert_eq!(
            parse_command("diff -n 0x80 other.bin").unwrap(),
            Command::Diff(DiffCommand::Open {
                path: PathBuf::from("other.bin"),
                max_shift: Some(0x80),
            })
        );
        assert_eq!(
            parse_command("diff refresh").unwrap(),
            Command::Diff(DiffCommand::Refresh)
        );
        assert_eq!(
            parse_command("diff next").unwrap(),
            Command::Diff(DiffCommand::Next)
        );
        assert_eq!(
            parse_command("diff prev").unwrap(),
            Command::Diff(DiffCommand::Prev)
        );
        assert_eq!(
            parse_command("diff off").unwrap(),
            Command::Diff(DiffCommand::Off)
        );
    }

    #[test]
    fn format_command_accepts_optional_name() {
        assert_eq!(
            parse_command("format").unwrap(),
            Command::Format { name: None }
        );
        assert_eq!(
            parse_command("format elf").unwrap(),
            Command::Format {
                name: Some("elf".to_owned())
            }
        );
    }

    #[test]
    fn goto_command_accepts_end_and_relative_offsets() {
        assert_eq!(
            parse_command("goto end").unwrap(),
            Command::Goto {
                target: GotoTarget::End
            }
        );
        assert_eq!(
            parse_command("goto +0x10").unwrap(),
            Command::Goto {
                target: GotoTarget::Relative(0x10)
            }
        );
        assert_eq!(
            parse_command("goto -20").unwrap(),
            Command::Goto {
                target: GotoTarget::Relative(-20)
            }
        );
    }

    #[test]
    fn redo_command_accepts_optional_steps() {
        assert_eq!(parse_command("redo").unwrap(), Command::Redo { steps: 1 });
        assert_eq!(parse_command("redo 3").unwrap(), Command::Redo { steps: 3 });
    }

    #[test]
    fn source_command_requires_path() {
        assert_eq!(
            parse_command("source patch.hxmacro").unwrap(),
            Command::Source {
                path: PathBuf::from("patch.hxmacro")
            }
        );
        assert!(matches!(
            parse_command("source"),
            Err(HxError::MissingArgument("macro path"))
        ));
    }

    #[test]
    fn script_command_requires_path() {
        assert_eq!(
            parse_command("script patch.hxscript").unwrap(),
            Command::Script {
                path: PathBuf::from("patch.hxscript")
            }
        );
        assert!(matches!(
            parse_command("script"),
            Err(HxError::MissingArgument("script path"))
        ));
    }

    #[test]
    fn hash_command_parses_all_algorithms() {
        assert_eq!(
            parse_command("hash md5").unwrap(),
            Command::Hash {
                algorithm: HashAlgorithm::Md5
            }
        );
        assert_eq!(
            parse_command("hash sha1").unwrap(),
            Command::Hash {
                algorithm: HashAlgorithm::Sha1
            }
        );
        assert_eq!(
            parse_command("hash sha256").unwrap(),
            Command::Hash {
                algorithm: HashAlgorithm::Sha256
            }
        );
        assert_eq!(
            parse_command("hash sha512").unwrap(),
            Command::Hash {
                algorithm: HashAlgorithm::Sha512
            }
        );
        assert_eq!(
            parse_command("hash crc32").unwrap(),
            Command::Hash {
                algorithm: HashAlgorithm::Crc32
            }
        );
    }

    #[test]
    fn hash_command_rejects_unknown_algorithm() {
        let err = parse_command("hash blake2").unwrap_err();
        assert!(err.to_string().contains("blake2"));
    }

    #[test]
    fn hash_command_requires_algorithm_argument() {
        let err = parse_command("hash").unwrap_err();
        assert!(err.to_string().contains("hash algorithm"));
    }

    #[test]
    fn stats_command_parses_modes() {
        assert_eq!(
            parse_command("stats").unwrap(),
            Command::Stats(StatsCommand::Auto)
        );
        assert_eq!(
            parse_command("stat all").unwrap(),
            Command::Stats(StatsCommand::All)
        );
        assert_eq!(
            parse_command("stats selection").unwrap(),
            Command::Stats(StatsCommand::Selection)
        );
        assert_eq!(
            parse_command("stats sel").unwrap(),
            Command::Stats(StatsCommand::Selection)
        );
        assert_eq!(
            parse_command("stats refresh").unwrap(),
            Command::Stats(StatsCommand::Refresh)
        );
        assert_eq!(
            parse_command("stats off").unwrap(),
            Command::Stats(StatsCommand::Off)
        );
        assert!(parse_command("stats nope").is_err());
    }

    #[test]
    fn xor_command_parses_copy_and_in_place_variants() {
        assert_eq!(
            parse_command("xor 0xaa").unwrap(),
            Command::Xor {
                key: 0xaa,
                in_place: false,
            }
        );
        assert_eq!(
            parse_command("xor! 170").unwrap(),
            Command::Xor {
                key: 0xaa,
                in_place: true,
            }
        );
        assert_eq!(
            parse_command("xor 15").unwrap(),
            Command::Xor {
                key: 0x0f,
                in_place: false,
            }
        );
    }

    #[test]
    fn xor_command_rejects_non_byte_keys() {
        assert!(matches!(
            parse_command("xor 0x123"),
            Err(HxError::InvalidXorKey(_))
        ));
        assert!(matches!(
            parse_command("xor zz"),
            Err(HxError::InvalidXorKey(_))
        ));
        assert!(matches!(
            parse_command("xor 256"),
            Err(HxError::InvalidXorKey(_))
        ));
    }

    #[cfg(feature = "disasm")]
    #[test]
    fn disassembly_commands_parse_when_feature_enabled() {
        assert_eq!(
            parse_command("dis").unwrap(),
            Command::Disassemble { arch: None }
        );
        assert_eq!(parse_command("dis off").unwrap(), Command::DisassembleOff);
        assert_eq!(
            parse_command("si ret").unwrap(),
            Command::SearchInstruction {
                pattern: "ret".to_owned(),
                backward: false,
            }
        );
    }

    #[cfg(not(feature = "disasm"))]
    #[test]
    fn disassembly_commands_are_unknown_when_feature_disabled() {
        assert!(matches!(
            parse_command("dis"),
            Err(HxError::UnknownCommand(name)) if name == "dis"
        ));
        assert!(matches!(
            parse_command("si ret"),
            Err(HxError::UnknownCommand(name)) if name == "si"
        ));
    }

    #[cfg(feature = "symbols")]
    #[test]
    fn symbol_commands_parse_when_feature_enabled() {
        assert_eq!(parse_command("sym").unwrap(), Command::Symbols);
        assert_eq!(parse_command("sym off").unwrap(), Command::SymbolsOff);
        assert_eq!(
            parse_command("symbol entry").unwrap(),
            Command::SearchSymbol {
                pattern: "entry".to_owned(),
                backward: false,
            }
        );
        assert_eq!(
            parse_command("symbol! entry").unwrap(),
            Command::SearchSymbol {
                pattern: "entry".to_owned(),
                backward: true,
            }
        );
        assert_eq!(
            parse_command("search-symbol entry").unwrap(),
            Command::SearchSymbol {
                pattern: "entry".to_owned(),
                backward: false,
            }
        );
        assert_eq!(
            parse_command("search-symbol! entry").unwrap(),
            Command::SearchSymbol {
                pattern: "entry".to_owned(),
                backward: true,
            }
        );
    }

    #[cfg(not(feature = "symbols"))]
    #[test]
    fn symbol_commands_are_unknown_when_feature_disabled() {
        assert!(matches!(
            parse_command("sym"),
            Err(HxError::UnknownCommand(name)) if name == "sym"
        ));
        assert!(matches!(
            parse_command("symbol entry"),
            Err(HxError::UnknownCommand(name)) if name == "symbol"
        ));
    }

    #[cfg(feature = "sagitta-analysis")]
    #[test]
    fn analysis_commands_parse_when_feature_enabled() {
        use crate::commands::types::AnalysisCommand;

        assert_eq!(
            parse_command("ana").unwrap(),
            Command::Analysis(AnalysisCommand::Run)
        );
        assert_eq!(
            parse_command("ana status").unwrap(),
            Command::Analysis(AnalysisCommand::Status)
        );
        assert_eq!(
            parse_command("analysis off").unwrap(),
            Command::Analysis(AnalysisCommand::Off)
        );
    }

    #[cfg(not(feature = "sagitta-analysis"))]
    #[test]
    fn analysis_commands_are_unknown_when_feature_disabled() {
        assert!(matches!(
            parse_command("ana"),
            Err(HxError::UnknownCommand(name)) if name == "ana"
        ));
    }
}
