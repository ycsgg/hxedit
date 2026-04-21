use std::path::PathBuf;

use crate::commands::{
    split_command,
    types::{Command, ExportFormat, GotoTarget, HashAlgorithm},
};
use crate::copy::{CopyDisplay, CopyFormat};
use crate::error::{HxError, HxResult};
use crate::util::parse::{parse_hex_bytes, parse_hex_stream, parse_offset};

const DEFAULT_EXPORT_NAME: &str = "selection_bytes";

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
        "q" | "quit" => Ok(Command::Quit { force: false }),
        "q!" | "quit!" => Ok(Command::Quit { force: true }),
        "w" | "write" => Ok(Command::Write {
            path: opt_path(rest),
        }),
        "wq" => Ok(Command::WriteQuit {
            path: opt_path(rest),
        }),
        "fill" => parse_fill(rest),
        "zero" => parse_zero(rest),
        "re" | "replace" | "re!" | "replace!" => parse_replace(name, rest),
        "p" | "paste" | "p!" | "paste!" | "p?" | "paste?" | "p!?" | "p?!" | "paste!?"
        | "paste?!" => parse_paste(name, rest, false),
        "pi" | "paste-insert" | "pi!" | "paste-insert!" | "pi?" | "paste-insert?" | "pi!?"
        | "pi?!" | "paste-insert!?" | "paste-insert?!" => parse_paste(name, rest, true),
        "c" | "copy" => parse_copy(rest),
        "export" => parse_export(rest),
        "u" | "undo" => Ok(Command::Undo {
            steps: parse_undo_steps(rest)?,
        }),
        "redo" => Ok(Command::Redo {
            steps: parse_redo_steps(rest)?,
        }),
        "insp" | "inspector" => match rest.map(str::trim) {
            None | Some("") => Ok(Command::Inspector),
            Some("more") => Ok(Command::InspectorMore),
            Some(other) => Err(HxError::UnknownCommand(format!("insp {other}"))),
        },
        "format" => Ok(Command::Format {
            name: rest.filter(|value| !value.is_empty()).map(str::to_owned),
        }),
        "g" | "goto" => {
            let arg = rest.ok_or(HxError::MissingArgument("offset"))?;
            Ok(Command::Goto {
                target: parse_goto_target(arg)?,
            })
        }
        "s" | "s!" => {
            let arg = rest.ok_or(HxError::MissingArgument("ascii search pattern"))?;
            if arg.is_empty() {
                return Err(HxError::EmptySearch);
            }
            Ok(Command::SearchAscii {
                pattern: arg.as_bytes().to_vec(),
                backward: name.ends_with('!'),
            })
        }
        "S" | "S!" => {
            let arg = rest.ok_or(HxError::MissingArgument("hex search pattern"))?;
            Ok(Command::SearchHex {
                pattern: parse_hex_bytes(arg)?,
                backward: name.ends_with('!'),
            })
        }
        "hash" => {
            let arg = rest.ok_or(HxError::MissingArgument("hash algorithm"))?;
            let algo = HashAlgorithm::parse(arg)
                .ok_or_else(|| HxError::InvalidHashAlgorithm(arg.to_owned()))?;
            Ok(Command::Hash { algorithm: algo })
        }
        "dis" | "disassemble" => match rest.map(str::trim) {
            None | Some("") => Ok(Command::Disassemble { arch: None }),
            Some("off") => Ok(Command::DisassembleOff),
            Some(arg) => Ok(Command::Disassemble {
                arch: Some(arg.to_owned()),
            }),
        },
        other => Err(HxError::UnknownCommand(other.to_owned())),
    }
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

fn opt_path(input: Option<&str>) -> Option<PathBuf> {
    input.filter(|s| !s.is_empty()).map(PathBuf::from)
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
            let name = tokens.next().unwrap_or(DEFAULT_EXPORT_NAME);
            if let Some(extra) = tokens.next() {
                return Err(HxError::UnknownCommand(extra.to_owned()));
            }
            ExportFormat::CArray {
                name: name.to_owned(),
            }
        }
        "py" | "python" => {
            let name = tokens.next().unwrap_or(DEFAULT_EXPORT_NAME);
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

fn parse_replace(name: &str, input: Option<&str>) -> HxResult<Command> {
    let allow_resize = name.ends_with('!');
    let rest = input.ok_or(HxError::MissingArgument("replace arguments"))?;
    let (mode, body) = parse_replace_mode(rest);
    let (needle_src, replacement_src) = body
        .split_once("->")
        .or_else(|| body.split_once("=>"))
        .ok_or_else(|| HxError::InvalidReplace("expected <needle> -> <replacement>".to_owned()))?;

    let needle = parse_replace_bytes(mode, needle_src.trim())?;
    let replacement = parse_replace_bytes(mode, replacement_src.trim())?;

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
    })
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
    fn inspector_aliases_parse() {
        assert_eq!(parse_command("insp").unwrap(), Command::Inspector);
        assert_eq!(parse_command("inspector").unwrap(), Command::Inspector);
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
}
