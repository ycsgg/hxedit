pub mod hints;
pub mod parser;
pub mod types;

pub(crate) const QUIT_ALIASES: &[&str] = &["q", "quit"];
pub(crate) const QUIT_FORCE_ALIASES: &[&str] = &["q!", "quit!"];
pub(crate) const WRITE_ALIASES: &[&str] = &["w", "write"];
pub(crate) const WRITE_QUIT_ALIASES: &[&str] = &["wq"];
pub(crate) const FILL_ALIASES: &[&str] = &["fill"];
pub(crate) const ZERO_ALIASES: &[&str] = &["zero"];
pub(crate) const REPLACE_ALIASES: &[&str] = &["re", "replace", "re!", "replace!"];
pub(crate) const PASTE_ALIASES: &[&str] = &[
    "p", "paste", "p!", "paste!", "p?", "paste?", "p!?", "p?!", "paste!?", "paste?!",
];
pub(crate) const PASTE_INSERT_ALIASES: &[&str] = &[
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
];
pub(crate) const COPY_ALIASES: &[&str] = &["c", "copy"];
pub(crate) const EXPORT_ALIASES: &[&str] = &["export"];
pub(crate) const XOR_ALIASES: &[&str] = &["xor", "xor!"];
pub(crate) const UNDO_ALIASES: &[&str] = &["u", "undo"];
pub(crate) const REDO_ALIASES: &[&str] = &["redo"];
pub(crate) const SOURCE_ALIASES: &[&str] = &["source"];
pub(crate) const SCRIPT_ALIASES: &[&str] = &["script"];
pub(crate) const INSPECTOR_ALIASES: &[&str] = &["insp", "inspector"];
pub(crate) const FORMAT_ALIASES: &[&str] = &["format"];
pub(crate) const GOTO_ALIASES: &[&str] = &["g", "goto"];
pub(crate) const SEARCH_ALIASES: &[&str] = &["s", "s!"];
pub(crate) const LEGACY_HEX_SEARCH_ALIASES: &[&str] = &["S", "S!"];
pub(crate) const HASH_ALIASES: &[&str] = &["hash"];
pub(crate) const DIFF_ALIASES: &[&str] = &["diff"];
pub(crate) const DATA_ALIASES: &[&str] = &["data"];

#[cfg(feature = "memory")]
pub(crate) const MEMORY_ALIASES: &[&str] = &["mem"];
#[cfg(feature = "memory")]
pub(crate) const MEMORY_SEARCH_ALIASES: &[&str] = &["ms", "ms!"];

#[cfg(feature = "disasm")]
pub(crate) const INSTRUCTION_SEARCH_ALIASES: &[&str] = &[
    "si",
    "si!",
    "search-instruction",
    "search-instruction!",
    "search-insn",
    "search-insn!",
];
#[cfg(feature = "disasm")]
pub(crate) const DISASSEMBLE_ALIASES: &[&str] = &["dis", "disassemble"];
#[cfg(feature = "disasm")]
pub(crate) const DISASSEMBLE_FORCE_ALIASES: &[&str] = &["dis!", "disassemble!"];

#[cfg(feature = "symbols")]
pub(crate) const SYMBOL_PANEL_ALIASES: &[&str] = &["sym", "symbols"];
#[cfg(feature = "symbols")]
pub(crate) const SYMBOL_SEARCH_ALIASES: &[&str] =
    &["symbol", "symbol!", "search-symbol", "search-symbol!"];

#[cfg(feature = "sagitta-analysis")]
pub(crate) const ANALYSIS_ALIASES: &[&str] = &["ana", "analysis"];

const COMMON_ALIAS_GROUPS: &[&[&str]] = &[
    QUIT_ALIASES,
    QUIT_FORCE_ALIASES,
    WRITE_ALIASES,
    WRITE_QUIT_ALIASES,
    FILL_ALIASES,
    ZERO_ALIASES,
    REPLACE_ALIASES,
    PASTE_ALIASES,
    PASTE_INSERT_ALIASES,
    COPY_ALIASES,
    EXPORT_ALIASES,
    XOR_ALIASES,
    UNDO_ALIASES,
    REDO_ALIASES,
    SOURCE_ALIASES,
    SCRIPT_ALIASES,
    INSPECTOR_ALIASES,
    FORMAT_ALIASES,
    GOTO_ALIASES,
    SEARCH_ALIASES,
    LEGACY_HEX_SEARCH_ALIASES,
    HASH_ALIASES,
    DIFF_ALIASES,
    DATA_ALIASES,
];

pub(crate) fn is_alias(name: &str, aliases: &[&str]) -> bool {
    aliases.contains(&name)
}

pub(crate) fn known_command_aliases() -> Vec<&'static str> {
    let mut commands = Vec::new();
    for group in COMMON_ALIAS_GROUPS {
        commands.extend(group.iter().copied());
    }
    #[cfg(feature = "disasm")]
    {
        commands.extend(INSTRUCTION_SEARCH_ALIASES.iter().copied());
        commands.extend(DISASSEMBLE_ALIASES.iter().copied());
        commands.extend(DISASSEMBLE_FORCE_ALIASES.iter().copied());
    }
    #[cfg(feature = "symbols")]
    {
        commands.extend(SYMBOL_PANEL_ALIASES.iter().copied());
        commands.extend(SYMBOL_SEARCH_ALIASES.iter().copied());
    }
    #[cfg(feature = "sagitta-analysis")]
    {
        commands.extend(ANALYSIS_ALIASES.iter().copied());
    }
    #[cfg(feature = "memory")]
    {
        commands.extend(MEMORY_ALIASES.iter().copied());
        commands.extend(MEMORY_SEARCH_ALIASES.iter().copied());
    }
    commands
}

pub(crate) fn split_command(input: &str) -> (&str, Option<&str>) {
    if let Some(idx) = input.find(char::is_whitespace) {
        let (name, tail) = input.split_at(idx);
        (name, Some(tail.trim()))
    } else {
        (input, None)
    }
}
