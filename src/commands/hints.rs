#[cfg(feature = "sagitta-analysis")]
use super::ANALYSIS_ALIASES;
use super::{
    is_alias, known_command_aliases, COPY_ALIASES, DATA_ALIASES, DIFF_ALIASES, EXPORT_ALIASES,
    FILL_ALIASES, FORMAT_ALIASES, GOTO_ALIASES, HASH_ALIASES, INSPECTOR_ALIASES,
    LEGACY_HEX_SEARCH_ALIASES, PASTE_ALIASES, PASTE_INSERT_ALIASES, QUIT_ALIASES,
    QUIT_FORCE_ALIASES, REDO_ALIASES, REPLACE_ALIASES, SEARCH_ALIASES, UNDO_ALIASES, WRITE_ALIASES,
    WRITE_QUIT_ALIASES, XOR_ALIASES, ZERO_ALIASES,
};
#[cfg(feature = "disasm")]
use super::{DISASSEMBLE_ALIASES, DISASSEMBLE_FORCE_ALIASES, INSTRUCTION_SEARCH_ALIASES};
#[cfg(feature = "memory")]
use super::{MEMORY_ALIASES, MEMORY_SEARCH_ALIASES};
#[cfg(feature = "symbols")]
use super::{SYMBOL_PANEL_ALIASES, SYMBOL_SEARCH_ALIASES};
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
        name if is_alias(name, QUIT_ALIASES) || is_alias(name, QUIT_FORCE_ALIASES) => CommandHint {
            syntax: "q | quit | q! | quit!".to_owned(),
            details: "quit editor; ! forces quit even with unsaved changes".to_owned(),
        },
        name if is_alias(name, WRITE_ALIASES) => CommandHint {
            syntax: "w [path] | write [path]".to_owned(),
            details: "save current file; optional path writes to a new target".to_owned(),
        },
        name if is_alias(name, WRITE_QUIT_ALIASES) => CommandHint {
            syntax: "wq".to_owned(),
            details: "save current file and quit".to_owned(),
        },
        name if is_alias(name, FILL_ALIASES) => CommandHint {
            syntax: "fill <hex-pattern> <len>".to_owned(),
            details: "overwrite bytes from cursor with a repeated hex pattern; len is the number of bytes to write".to_owned(),
        },
        name if is_alias(name, ZERO_ALIASES) => CommandHint {
            syntax: "zero <len>".to_owned(),
            details: "overwrite bytes from cursor with 0x00 for len bytes".to_owned(),
        },
        name if is_alias(name, REPLACE_ALIASES) => CommandHint {
            syntax: format!("{name} [--force] [mode]<delim><needle><delim><replacement><delim>"),
            details: if name.ends_with('!') {
                "replace all non-overlapping matches in the active selection (visual or selected inspector field) or entire file; ! allows length changes via real delete/insert"
                    .to_owned()
            } else {
                "replace all non-overlapping matches with equal-length data; modes match :s (/text/, x/hex/, b/byte/, u32/u64 and signed variants); --force applies when more than 65535 matches are found"
                    .to_owned()
            },
        },
        name if is_alias(name, INSPECTOR_ALIASES) => {
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
                    "show or focus the inspector page in the side panel; `:insp more` reveals the next batch of paginated entries when a format uses capped lists".to_owned()
                },
            }
        }
        name if is_alias(name, FORMAT_ALIASES) => format_hint(),
        #[cfg(feature = "memory")]
        name if is_alias(name, MEMORY_ALIASES) => CommandHint {
            syntax: "mem [list|refresh|info|freeze|thaw|commit|commit-all]".to_owned(),
            details: "open/focus memory panel, list processes, refresh maps, inspect state, freeze/thaw the target, or commit active memory-document replacements".to_owned(),
        },
        name if is_alias(name, PASTE_ALIASES) => paste_hint(name, rest, false),
        name if is_alias(name, PASTE_INSERT_ALIASES) => paste_hint(name, rest, true),
        name if is_alias(name, GOTO_ALIASES) => CommandHint {
            syntax: format!("{name} <offset|end|+delta|-delta>"),
            details:
                "jump to an absolute offset, end, or a relative delta; supports decimal or 0x-prefixed hex, and reports the moved byte delta on success"
                    .to_owned(),
        },
        name if is_alias(name, SEARCH_ALIASES) => CommandHint {
            syntax: format!("{name} [mode]<delim><pattern><delim>"),
            details: if name.ends_with('!') {
                "search upward; modes include /text/, x/hex/, b/byte/, u32/u64 and signed variants; use n/p to jump next/previous match".to_owned()
            } else {
                "search downward; modes include /text/, x/hex/, b/byte/, u32/u64 and signed variants; use n/p to jump next/previous match".to_owned()
            },
        },
        name if is_alias(name, LEGACY_HEX_SEARCH_ALIASES) => CommandHint {
            syntax: format!("{name} <hex-bytes> (deprecated; use s{} x/<hex-bytes>/)", if name.ends_with('!') { "!" } else { "" }),
            details: if name.ends_with('!') {
                "deprecated hex-search alias; search upward with the unified form like: s! x/7f 45 4c 46/".to_owned()
            } else {
                "deprecated hex-search alias; search downward with the unified form like: s x/7f 45 4c 46/".to_owned()
            },
        },
        #[cfg(feature = "memory")]
        name if is_alias(name, MEMORY_SEARCH_ALIASES) => CommandHint {
            syntax: format!("{name} [mode]<delim><pattern><delim> [in:<selector>] [not:<selector>]"),
            details: if name.ends_with('!') {
                "search process memory upward across readable regions; modes include /text/, x/hex/, b/byte/, u32/u64, filters include permissions, kind, path glob, and va range; repeat with gn/gN".to_owned()
            } else {
                "search process memory downward across readable regions; modes include /text/, x/hex/, b/byte/, u32/u64, filters include permissions, kind, path glob, and va range; repeat with gn/gN".to_owned()
            },
        },
        #[cfg(feature = "disasm")]
        name if is_alias(name, INSTRUCTION_SEARCH_ALIASES) => CommandHint {
            syntax: format!("{name} <instruction-text>"),
            details: if name.ends_with('!') {
                "search decoded instruction text upward in disassembly view; matches mnemonic and operands, then jumps to the matching instruction row".to_owned()
            } else {
                "search decoded instruction text downward in disassembly view; matches mnemonic and operands, then jumps to the matching instruction row".to_owned()
            },
        },
        #[cfg(feature = "symbols")]
        name if is_alias(name, SYMBOL_SEARCH_ALIASES) => CommandHint {
            syntax: format!("{name} <symbol-name>"),
            details: if name.ends_with('!') {
                "search symbolized disassembly rows upward in disassembly view; matches symbol labels, symbolized operands, and direct-target symbol hints, then jumps to the matching row".to_owned()
            } else {
                "search symbolized disassembly rows downward in disassembly view; matches symbol labels, symbolized operands, and direct-target symbol hints, then jumps to the matching row".to_owned()
            },
        },
        name if is_alias(name, UNDO_ALIASES) => CommandHint {
            syntax: format!("{name} [steps]"),
            details: "undo one change by default; pass a positive number to undo more".to_owned(),
        },
        name if is_alias(name, REDO_ALIASES) => CommandHint {
            syntax: "redo [steps]".to_owned(),
            details: "redo one undone change by default; pass a positive number to redo more"
                .to_owned(),
        },
        name if is_alias(name, COPY_ALIASES) => copy_hint(name, rest),
        name if is_alias(name, EXPORT_ALIASES) => CommandHint {
            syntax: "export <path> | export bin <path> | export c [name] | export py [name]"
                .to_owned(),
            details:
                "export the active selection (visual or selected inspector field) as raw bytes to a file, or copy a C/Python literal to the clipboard"
                    .to_owned(),
        },
        name if is_alias(name, XOR_ALIASES) => CommandHint {
            syntax: format!("{name} <0x??|0..255>"),
            details: if name.ends_with('!') {
                "xor each logical byte in the active selection with a one-byte key, then overwrite the same display cells in place; bare keys are decimal, 0x-prefixed keys are hex"
                    .to_owned()
            } else {
                "xor the active selection with a one-byte key and copy the resulting hex bytes to the clipboard without editing the file; bare keys are decimal, 0x-prefixed keys are hex"
                    .to_owned()
            },
        },
        name if is_alias(name, HASH_ALIASES) => CommandHint {
            syntax: "hash <md5|sha1|sha256|sha512|crc32>".to_owned(),
            details: "compute hash of the current selection (visual or selected inspector field), or the entire file if no selection is active".to_owned(),
        },
        name if is_alias(name, DIFF_ALIASES) => {
            let syntax = match rest.map(str::trim) {
                Some("off") => "diff off".to_owned(),
                Some("refresh") => "diff refresh".to_owned(),
                Some("next") => "diff next".to_owned(),
                Some("prev") => "diff prev".to_owned(),
                Some(arg) if arg.starts_with("-n") => "diff -n <N> <path>".to_owned(),
                _ => "diff <path> | diff -n <N> <path> | diff refresh|next|prev|off"
                    .to_owned(),
            };
            CommandHint {
                syntax,
                details:
                    "show a read-only synchronized page comparing current logical bytes with another file; next/prev scan in large progress-reporting steps, block other input, and Esc cancels"
                        .to_owned(),
            }
        }
        #[cfg(feature = "sagitta-analysis")]
        name if is_alias(name, ANALYSIS_ALIASES) => {
            let syntax = match rest.map(str::trim) {
                Some("status") => "ana status".to_owned(),
                Some("off") => "ana off".to_owned(),
                _ => "ana | ana status | ana off".to_owned(),
            };
            CommandHint {
                syntax,
                details: "run Sagitta analysis on current logical bytes, inspect analysis status, or clear the Sagitta snapshot".to_owned(),
            }
        }
        #[cfg(feature = "disasm")]
        name if is_alias(name, DISASSEMBLE_ALIASES) => {
            let syntax = match rest.map(str::trim) {
                Some("off") => "dis off".to_owned(),
                Some(arg) if !arg.is_empty() => format!("{name} {arg}"),
                _ => "dis [x86|x86_64|arm64|riscv64|off]".to_owned(),
            };
            CommandHint {
                syntax,
                details: "enter the read-only disassembly main view for ELF/PE/Mach-O using detected executable metadata and the current decode backend; `dis off` returns to hex view".to_owned(),
            }
        }
        #[cfg(feature = "disasm")]
        name if is_alias(name, DISASSEMBLE_FORCE_ALIASES) => CommandHint {
            syntax: format!("{name} <x86|x86_64|arm64|riscv64> <offset>"),
            details: "force a raw disassembly view from the given display offset even when the file is not recognized as ELF/PE/Mach-O; assumes little-endian decoding for the chosen arch".to_owned(),
        },
        #[cfg(feature = "symbols")]
        name if is_alias(name, SYMBOL_PANEL_ALIASES) => {
            let syntax = match rest.map(str::trim) {
                Some("off") => "sym off".to_owned(),
                Some(arg) if !arg.is_empty() => format!("{name} {arg}"),
                _ => "sym | symbols | sym off".to_owned(),
            };
            CommandHint {
                syntax,
                details: "show executable symbols/import targets in the side panel; `sym off` closes the symbol page and restores inspector when available".to_owned(),
            }
        }
        name if is_alias(name, DATA_ALIASES) => {
            let syntax = match rest.map(str::trim) {
                Some("off") => "data off".to_owned(),
                _ => "data | data off".to_owned(),
            };
            CommandHint {
                syntax,
                details: "show cursor-relative primitive data decoding in the side panel; click a row to select its decoded bytes".to_owned(),
            }
        }
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

fn format_hint() -> CommandHint {
    let aliases = crate::format::detect::forced_format_alias_list();
    CommandHint {
        syntax: format!(
            "format [{}]",
            crate::format::detect::forced_format_primary_names()
        ),
        details: if aliases.is_empty() {
            "auto-detect format when omitted, or force a built-in inspector".to_owned()
        } else {
            format!(
                "auto-detect format when omitted, or force a built-in inspector; aliases: {aliases}"
            )
        },
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
    known_command_aliases()
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
    fn format_hint_uses_format_registry() {
        let hint = hint_for("format");
        assert!(hint.syntax.contains("sqlite"));
        assert!(hint.syntax.contains("pcapng"));
        assert!(hint.syntax.contains("macho"));
        assert!(hint.details.contains("sqlite3"));
        assert!(hint.details.contains("mach-o"));
    }

    #[test]
    fn completion_aliases_use_shared_registry() {
        let commands = known_commands();
        assert!(commands.contains(&"quit!"));
        assert!(commands.contains(&"paste-insert?!"));
        #[cfg(feature = "symbols")]
        {
            assert!(commands.contains(&"search-symbol"));
            assert!(commands.contains(&"search-symbol!"));
        }
        #[cfg(feature = "disasm")]
        {
            assert!(commands.contains(&"search-instruction"));
            assert!(commands.contains(&"disassemble!"));
        }
    }

    #[cfg(feature = "symbols")]
    #[test]
    fn symbol_hint_mentions_side_panel() {
        let hint = hint_for("sym");
        assert!(hint.syntax.contains("sym off"));
        assert!(hint.details.contains("side panel"));
    }

    #[cfg(feature = "symbols")]
    #[test]
    fn symbol_search_hint_mentions_disassembly_rows() {
        let hint = hint_for("symbol");
        assert!(hint.syntax.contains("<symbol-name>"));
        assert!(hint.details.contains("symbolized"));
    }

    #[cfg(not(feature = "symbols"))]
    #[test]
    fn symbol_hint_hidden_when_feature_disabled() {
        let hint = hint_for("sym");
        assert_eq!(hint.syntax, "unknown command");
    }

    #[cfg(feature = "sagitta-analysis")]
    #[test]
    fn analysis_hint_mentions_status_and_off() {
        let hint = hint_for("ana");
        assert!(hint.syntax.contains("ana status"));
        assert!(hint.syntax.contains("ana off"));
        assert!(hint.details.contains("Sagitta"));
    }

    #[test]
    fn data_hint_mentions_row_selection() {
        let hint = hint_for("data");
        assert!(hint.syntax.contains("data off"));
        assert!(hint.details.contains("select"));
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
    fn xor_hint_mentions_copy_and_in_place_modes() {
        let hint = hint_for("xor");
        assert!(hint.syntax.contains("0x??"));
        assert!(hint.syntax.contains("0..255"));
        assert!(hint.details.contains("decimal"));
        assert!(hint.details.contains("clipboard"));

        let in_place = hint_for("xor!");
        assert!(in_place.details.contains("in place"));
    }

    #[test]
    fn replace_hint_mentions_mode_and_resize_mode() {
        let hint = hint_for("re!");
        assert!(hint.syntax.contains("[mode]<delim>"));
        assert!(hint.details.contains("length changes"));
    }
}
