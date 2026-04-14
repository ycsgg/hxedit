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
        "p" | "paste" | "p!" | "paste!" | "p?" | "paste?" | "p!?" | "p?!" | "paste!?"
        | "paste?!" => paste_hint(name, rest, false),
        "pi" | "paste-insert" | "pi!" | "paste-insert!" | "pi?" | "paste-insert?" | "pi!?"
        | "pi?!" | "paste-insert!?" | "paste-insert?!" => paste_hint(name, rest, true),
        "g" | "goto" => CommandHint {
            syntax: format!("{name} <offset>"),
            details: "jump to byte offset; supports decimal or 0x-prefixed hex".to_owned(),
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
        "u" | "undo" => CommandHint {
            syntax: format!("{name} [steps]"),
            details: "undo one change by default; pass a positive number to undo more".to_owned(),
        },
        "c" | "copy" => copy_hint(name, rest),
        other => {
            let suggestions = known_commands()
                .into_iter()
                .filter(|candidate| candidate.starts_with(other))
                .collect::<Vec<_>>();
            if suggestions.is_empty() {
                CommandHint {
                    syntax: "unknown command".to_owned(),
                    details: "available: q w wq g s s! S S! u c p pi".to_owned(),
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
            if display.is_none() && matches!(token, "r" | "raw" | "nb" | "nl") {
                display = Some(token);
            }
        }
    }

    let remaining = match (format.is_some(), display.is_some()) {
        (false, false) => "[bin|b|db|qb] [r|nb|nl]",
        (true, false) => "[r|nb|nl]",
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
            "fmt: bin=binary b=byte(default) db=2-byte qb=4-byte; disp: r=raw(default) nb=big-endian nums nl=little-endian nums"
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
            "overwrite existing bytes from cursor. default parses as hex/base64. ! pastes raw bytes. num limits pasted bytes."
                .to_owned()
        },
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

fn known_commands() -> Vec<&'static str> {
    vec![
        "q",
        "quit",
        "q!",
        "w",
        "write",
        "wq",
        "g",
        "goto",
        "s",
        "s!",
        "S",
        "S!",
        "u",
        "undo",
        "c",
        "copy",
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
        assert_eq!(hint.syntax, "copy [r|nb|nl]");
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
}
