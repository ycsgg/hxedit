use std::path::PathBuf;

use crate::commands::types::Command;
use crate::error::{HxError, HxResult};
use crate::util::parse::{parse_hex_bytes, parse_offset};

/// Parse command-line mode input into an executable command.
pub fn parse_command(input: &str) -> HxResult<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(HxError::UnknownCommand(trimmed.to_owned()));
    }

    let (name, rest) = split_command(trimmed);
    match name {
        "q" | "quit" => Ok(Command::Quit { force: false }),
        "q!" | "quit!" => Ok(Command::Quit { force: true }),
        "w" | "write" => Ok(Command::Write {
            path: opt_path(rest),
        }),
        "wq" => Ok(Command::WriteQuit {
            path: opt_path(rest),
        }),
        "u" | "undo" => Ok(Command::Undo {
            steps: parse_undo_steps(rest)?,
        }),
        "g" | "goto" => {
            let arg = rest.ok_or(HxError::MissingArgument("offset"))?;
            Ok(Command::Goto {
                offset: parse_offset(arg).map_err(|_| HxError::InvalidOffset(arg.to_owned()))?,
            })
        }
        "s" => {
            let arg = rest.ok_or(HxError::MissingArgument("ascii search pattern"))?;
            if arg.is_empty() {
                return Err(HxError::EmptySearch);
            }
            Ok(Command::SearchAscii {
                pattern: arg.as_bytes().to_vec(),
            })
        }
        "S" => {
            let arg = rest.ok_or(HxError::MissingArgument("hex search pattern"))?;
            Ok(Command::SearchHex {
                pattern: parse_hex_bytes(arg)?,
            })
        }
        other => Err(HxError::UnknownCommand(other.to_owned())),
    }
}

fn split_command(input: &str) -> (&str, Option<&str>) {
    if let Some(idx) = input.find(char::is_whitespace) {
        let (name, tail) = input.split_at(idx);
        (name, Some(tail.trim()))
    } else {
        (input, None)
    }
}

fn opt_path(input: Option<&str>) -> Option<PathBuf> {
    input.filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn parse_undo_steps(input: Option<&str>) -> HxResult<usize> {
    match input {
        None => Ok(1),
        Some("") => Ok(1),
        Some(value) => {
            let steps = value
                .parse::<usize>()
                .map_err(|_| HxError::InvalidUndoCount(value.to_owned()))?;
            if steps == 0 {
                return Err(HxError::InvalidUndoCount(value.to_owned()));
            }
            Ok(steps)
        }
    }
}
