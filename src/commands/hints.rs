use crate::commands::split_command;

#[derive(Debug, Clone)]
pub struct CommandHint {
    pub syntax: String,
    pub details: String,
}

pub fn hint_for(input: &str) -> CommandHint {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return CommandHint {
            syntax: "type a command name".to_owned(),
            details: "hints expand after you start typing".to_owned(),
        };
    }

    let (name, rest) = split_command(trimmed);
    match name {
        "q" | "quit" | "q!" | "quit!" => CommandHint {
            syntax: "q | quit | q! | quit!".to_owned(),
            details: "quit editor; ! forces quit even with unsaved changes".to_owned(),
        },
        "w" | "write" => CommandHint {
            syntax: "w [path] | write [path]".to_owned(),
            details: "save current file; optional path writes to a new target".to_owned(),
        },
        "wq" => CommandHint {
            syntax: "wq".to_owned(),
            details: "save current file and quit".to_owned(),
        },
        "fill" => CommandHint {
            syntax: "fill <hex-pattern> <len>".to_owned(),
            details: "overwrite bytes from cursor with a repeated hex pattern; len is the number of bytes to write".to_owned(),
        },
        "zero" => CommandHint {
            syntax: "zero <len>".to_owned(),
            details: "overwrite bytes from cursor with 0x00 for len bytes".to_owned(),
        },
        "re" | "replace" | "re!" | "replace!" => CommandHint {
            syntax: format!("{name} [hex|ascii] <needle> -> <replacement>"),
            details: if name.ends_with('!') {
                "replace all non-overlapping matches in the active selection (visual or selected inspector field) or entire file; ! allows length changes via real delete/insert"
                    .to_owned()
            } else {
                "replace all non-overlapping matches with equal-length data; defaults to hex mode, or use ascii for text"
                    .to_owned()
            },
        },
        "insp" | "inspector" => {
            let is_more = rest.map(str::trim) == Some("more");
            CommandHint {
                syntax: if is_more {
                    "insp more".to_owned()
                } else {
                    "insp | inspector | insp more".to_owned()
                },
                details: if is_more {
                    "reveal the next batch of paginated inspector entries beyond the current cap"
                        .to_owned()
                } else {
                    "toggle format inspector panel; `:insp more` reveals the next batch of paginated entries when a format uses capped lists".to_owned()
                },
            }
        }
        "format" => CommandHint {
            syntax: "format [elf|pe|macho|png|zip|gzip|gif|bmp|wav|tar|jpeg]".to_owned(),
            details: "auto-detect format when omitted, or force a built-in inspector".to_owned(),
        },
        "p" | "paste" | "p!" | "paste!" | "p?" | "paste?" | "p!?" | "p?!" | "paste!?"
        | "paste?!" => paste_hint(name, rest, false),
        "pi" | "paste-insert" | "pi!" | "paste-insert!" | "pi?" | "paste-insert?" | "pi!?"
        | "pi?!" | "paste-insert!?" | "paste-insert?!" => paste_hint(name, rest, true),
        "g" | "goto" => CommandHint {
            syntax: format!("{name} <offset|end|+delta|-delta>"),
            details:
                "jump to an absolute offset, end, or a relative delta; supports decimal or 0x-prefixed hex, and reports the moved byte delta on success"
                    .to_owned(),
        },
        "s" | "s!" => CommandHint {
            syntax: format!("{name} <ascii>"),
            details: if name.ends_with('!') {
                "search ASCII text upward; use n/p to jump next/previous match".to_owned()
            } else {
                "search ASCII text downward; use n/p to jump next/previous match".to_owned()
            },
        },
        "S" | "S!" => CommandHint {
            syntax: format!("{name} <hex-bytes>"),
            details: if name.ends_with('!') {
                "search hex bytes upward like: S! 7f 45 4c 46".to_owned()
            } else {
                "search hex bytes downward like: S 7f 45 4c 46".to_owned()
            },
        },
        "si" | "si!" | "search-instruction" | "search-insn" => CommandHint {
            syntax: format!("{name} <instruction-text>"),
            details: if name.ends_with('!') {
                "search decoded instruction text upward in disassembly view; matches mnemonic and operands, then jumps to the matching instruction row".to_owned()
            } else {
                "search decoded instruction text downward in disassembly view; matches mnemonic and operands, then jumps to the matching instruction row".to_owned()
            },
        },
        "u" | "undo" => CommandHint {
            syntax: format!("{name} [steps]"),
            details: "undo one change by default; pass a positive number to undo more".to_owned(),
        },
        "redo" => CommandHint {
            syntax: "redo [steps]".to_owned(),
            details: "redo one undone change by default; pass a positive number to redo more"
                .to_owned(),
        },
        "c" | "copy" => copy_hint(name, rest),
        "export" => CommandHint {
            syntax: "export <path> | export bin <path> | export c [name] | export py [name]"
                .to_owned(),
            details:
                "export the active selection (visual or selected inspector field) as raw bytes to a file, or copy a C/Python literal to the clipboard"
                    .to_owned(),
        },
        "hash" => CommandHint {
            syntax: "hash <md5|sha1|sha256|sha512|crc32>".to_owned(),
            details: "compute hash of the current selection (visual or selected inspector field), or the entire file if no selection is active".to_owned(),
        },
        "dis" | "disassemble" => {
            let syntax = match rest.map(str::trim) {
                Some("off") => "dis off".to_owned(),
                Some(arg) if !arg.is_empty() => format!("{name} {arg}"),
                _ => "dis [x86|x86_64|arm|aarch64|riscv64|off]".to_owned(),
            };
            CommandHint {
                syntax,
                details: "enter the read-only disassembly main view for ELF/PE/Mach-O using detected executable metadata and the current decode backend; `dis off` returns to hex view".to_owned(),
            }
        }
        "dis!" | "disassemble!" => CommandHint {
            syntax: format!("{name} <x86|x86_64|arm|aarch64|riscv64> <offset>"),
            details: "force a raw disassembly view from the given display offset even when the file is not recognized as ELF/PE/Mach-O; assumes little-endian decoding for the chosen arch".to_owned(),
        },
        other => {
            let suggestions = known_commands()
                .into_iter()
                .filter(|candidate| candidate.starts_with(other))
                .collect::<Vec<_>>();
            if suggestions.is_empty() {
                CommandHint {
                    syntax: "unknown command".to_owned(),
                    details: format!("available: {}", known_commands().join(" ")),
                }
            } else {
                CommandHint {
                    syntax: suggestions.join(" | "),
                    details: "keep typing, then provide the arguments shown for that command"
                        .to_owned(),
                }
            }
        }
    }
}

fn copy_hint(name: &str, rest: Option<&str>) -> CommandHint {
    let mut format = None;
    let mut display = None;

    if let Some(rest) = rest {
        for token in rest.split_whitespace() {
            if format.is_none() && matches!(token, "bin" | "binary" | "b" | "byte" | "db" | "qb") {
                format = Some(token);
                continue;
            }
            if display.is_none() && matches!(token, "r" | "raw" | "nb" | "nl" | "b64" | "base64") {
                display = Some(token);
            }
        }
    }

    let remaining = match (format.is_some(), display.is_some()) {
        (false, false) => "[bin|b|db|qb] [r|nb|nl|b64]",
        (true, false) => "[r|nb|nl|b64]",
        (false, true) => "[bin|b|db|qb]",
        (true, true) => "",
    };

    let syntax = if remaining.is_empty() {
        if let Some(rest) = rest {
            format!("{name} {}", rest.trim())
        } else {
            name.to_owned()
        }
    } else {
        format!("{name} {remaining}")
    };

    CommandHint {
        syntax,
        details:
            "copy the active selection; fmt: bin=binary b=byte(default) db=2-byte qb=4-byte; disp: r=raw(default) nb=big-endian nums nl=little-endian nums b64=base64"
                .to_owned(),
    }
}

fn paste_hint(name: &str, rest: Option<&str>, insert: bool) -> CommandHint {
    let mut raw = name.contains('!');
    let preview = name.contains('?');
    let mut has_limit = false;

    if let Some(rest) = rest {
        for token in rest.split_whitespace() {
            if token == "!" {
                raw = true;
            } else if token.parse::<usize>().is_ok() {
                has_limit = true;
            }
        }
    }

    let mut syntax = if raw {
        format!("{name} [num]")
    } else if has_limit {
        format!("{name} {}", rest.unwrap_or_default().trim())
    } else {
        format!("{name} [!] [num]")
    };
    if syntax.ends_with(' ') {
        syntax.pop();
    }

    CommandHint {
        syntax,
        details: if insert {
            if preview {
                "insert-mode preview; default parses clipboard as hex/base64 text. ! previews raw bytes. num limits previewed bytes."
                    .to_owned()
            } else {
                "insert clipboard bytes at cursor, shifting data right. default parses as hex/base64. ! pastes raw. num limits bytes."
                    .to_owned()
            }
        } else if preview {
            "preview only; default parses clipboard as hex/base64 text. ! previews raw clipboard bytes. num limits previewed bytes."
                .to_owned()
        } else {
            "overwrite existing bytes from cursor. default parses as hex/base64. ! pastes raw bytes. num limits pasted bytes. bytes past EOF are dropped."
                .to_owned()
        },
    }
}

fn known_commands() -> Vec<&'static str> {
    vec![
        "q",
        "quit",
        "q!",
        "w",
        "write",
        "wq",
        "fill",
        "zero",
        "re",
        "replace",
        "re!",
        "replace!",
        "g",
        "goto",
        "s",
        "s!",
        "S",
        "S!",
        "si",
        "si!",
        "search-instruction",
        "search-instruction!",
        "search-insn",
        "search-insn!",
        "u",
        "undo",
        "redo",
        "insp",
        "inspector",
        "format",
        "c",
        "copy",
        "export",
        "hash",
        "dis",
        "dis!",
        "disassemble",
        "disassemble!",
        "p",
        "paste",
        "p!",
        "paste!",
        "p?",
        "paste?",
        "p!?",
        "p?!",
        "paste!?",
        "paste?!",
        "pi",
        "paste-insert",
        "pi!",
        "paste-insert!",
        "pi?",
        "paste-insert?",
        "pi!?",
        "pi?!",
        "paste-insert!?",
        "paste-insert?!",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_shows_short_placeholder() {
        let hint = hint_for("");
        assert_eq!(hint.syntax, "type a command name");
        assert_eq!(hint.details, "hints expand after you start typing");
    }

    #[test]
    fn copy_hint_shows_remaining_args() {
        let hint = hint_for("copy db");
        assert_eq!(hint.syntax, "copy [r|nb|nl|b64]");
    }

    #[test]
    fn paste_hint_explains_raw_mode() {
        let hint = hint_for("paste!");
        assert!(hint.details.contains("raw bytes"));
    }

    #[test]
    fn paste_preview_hint_mentions_preview() {
        let hint = hint_for("paste?");
        assert!(hint.details.contains("preview only"));
    }

    #[test]
    fn goto_hint_shows_offset_help() {
        let hint = hint_for("go");
        assert!(hint.syntax.contains("goto"));
    }

    #[test]
    fn reverse_search_hint_mentions_upward() {
        let hint = hint_for("s!");
        assert!(hint.details.contains("upward"));
    }

    #[test]
    fn redo_hint_mentions_redoing_changes() {
        let hint = hint_for("redo");
        assert!(hint.details.contains("redo"));
    }

    #[test]
    fn inspector_hint_mentions_panel() {
        let hint = hint_for("insp");
        assert!(hint.details.contains("inspector"));
    }

    #[test]
    fn hash_hint_shows_algorithm_options() {
        let hint = hint_for("hash");
        assert!(hint.syntax.contains("md5"));
        assert!(hint.syntax.contains("sha256"));
        assert!(hint.syntax.contains("crc32"));
        assert!(hint.details.contains("selection"));
    }

    #[test]
    fn replace_hint_mentions_ascii_and_resize_mode() {
        let hint = hint_for("re!");
        assert!(hint.syntax.contains("ascii"));
        assert!(hint.details.contains("length changes"));
    }
}
