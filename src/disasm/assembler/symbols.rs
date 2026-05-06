use crate::error::{HxError, HxResult};
use crate::executable::{ExecutableArch, ExecutableInfo, PatchSymbolLookup};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPatchSymbol {
    pub token: String,
    pub address: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssemblyStatement {
    pub statement: String,
    pub resolved_symbol: Option<ResolvedPatchSymbol>,
}

pub fn resolve_patch_symbols(
    info: &ExecutableInfo,
    arch: ExecutableArch,
    statement: &str,
) -> HxResult<ResolvedAssemblyStatement> {
    let trimmed = statement.trim();
    if trimmed.is_empty() {
        return Ok(ResolvedAssemblyStatement {
            statement: statement.to_owned(),
            resolved_symbol: None,
        });
    }

    let Some((mnemonic, operands)) = split_mnemonic_and_operands(trimmed) else {
        return Ok(ResolvedAssemblyStatement {
            statement: statement.to_owned(),
            resolved_symbol: None,
        });
    };

    if supports_direct_branch_symbol(arch, mnemonic) {
        if let Some(token) = direct_branch_symbol_operand(arch, operands) {
            let address = match info.lookup_patch_symbol(token) {
                PatchSymbolLookup::Resolved(address) => address,
                PatchSymbolLookup::Ambiguous(matches) => {
                    return Err(HxError::AssemblyError(format!(
                        "ambiguous patch symbol: {token} ({} matches)",
                        matches.len()
                    )));
                }
                PatchSymbolLookup::Missing => {
                    return Err(HxError::AssemblyError(format!(
                        "unknown patch symbol: {token}"
                    )));
                }
            };
            return Ok(ResolvedAssemblyStatement {
                statement: format!("{mnemonic} 0x{address:x}"),
                resolved_symbol: Some(ResolvedPatchSymbol {
                    token: token.to_owned(),
                    address,
                }),
            });
        }
    }

    if contains_patch_symbol_operand(arch, operands) {
        return Err(HxError::AssemblyError(
            "symbol patch only supports direct branch/call operands".to_owned(),
        ));
    }

    Ok(ResolvedAssemblyStatement {
        statement: statement.to_owned(),
        resolved_symbol: None,
    })
}

fn split_mnemonic_and_operands(statement: &str) -> Option<(&str, &str)> {
    let split_at = statement.find(|ch: char| ch.is_whitespace())?;
    let mnemonic = &statement[..split_at];
    let operands = statement[split_at..].trim();
    (!mnemonic.is_empty() && !operands.is_empty()).then_some((mnemonic, operands))
}

fn supports_direct_branch_symbol(arch: ExecutableArch, mnemonic: &str) -> bool {
    let mnemonic = mnemonic.to_ascii_lowercase();
    match arch {
        ExecutableArch::X86 | ExecutableArch::X86_64 => {
            mnemonic == "call" || mnemonic.starts_with('j')
        }
        ExecutableArch::AArch64 => {
            mnemonic == "b" || mnemonic == "bl" || mnemonic.starts_with("b.")
        }
        _ => false,
    }
}

fn direct_branch_symbol_operand(arch: ExecutableArch, operands: &str) -> Option<&str> {
    let mut split = operands.split(',').map(str::trim);
    let operand = split.next()?;
    if split.next().is_some() {
        return None;
    }
    classify_patch_symbol_operand(arch, operand)
}

fn contains_patch_symbol_operand(arch: ExecutableArch, operands: &str) -> bool {
    operands
        .split(',')
        .map(str::trim)
        .any(|operand| classify_patch_symbol_operand(arch, operand).is_some())
}

fn classify_patch_symbol_operand(arch: ExecutableArch, operand: &str) -> Option<&str> {
    let operand = operand.trim();
    if operand.is_empty() {
        return None;
    }
    if operand.contains(|ch: char| ch.is_whitespace())
        || operand.contains(['[', ']', '(', ')', '*'])
    {
        return None;
    }

    let token = operand.strip_prefix('#').unwrap_or(operand).trim();
    if token.is_empty() || is_numeric_operand(token) || is_register_operand(arch, token) {
        return None;
    }
    is_patch_symbol_token(token).then_some(token)
}

fn is_numeric_operand(token: &str) -> bool {
    let token = token.strip_prefix('#').unwrap_or(token);
    let token = token.strip_prefix('+').unwrap_or(token);
    let token = token.strip_prefix('-').unwrap_or(token);
    if token.is_empty() {
        return false;
    }
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit());
    }
    token.chars().all(|ch| ch.is_ascii_digit())
}

fn is_patch_symbol_token(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, 'A'..='Z' | 'a'..='z' | '_' | '.' | '$' | '?' | '@') {
        return false;
    }
    chars.all(|ch| {
        matches!(
            ch,
            'A'..='Z'
                | 'a'..='z'
                | '0'..='9'
                | '_'
                | '.'
                | '$'
                | '?'
                | '@'
                | ':'
                | '-'
        )
    })
}

fn is_register_operand(arch: ExecutableArch, token: &str) -> bool {
    match arch {
        ExecutableArch::X86 | ExecutableArch::X86_64 => is_x86_register(token),
        ExecutableArch::AArch64 => is_aarch64_register(token),
        _ => false,
    }
}

fn is_x86_register(token: &str) -> bool {
    let token = token.to_ascii_lowercase();
    matches!(
        token.as_str(),
        "al" | "ah"
            | "ax"
            | "eax"
            | "rax"
            | "bl"
            | "bh"
            | "bx"
            | "ebx"
            | "rbx"
            | "cl"
            | "ch"
            | "cx"
            | "ecx"
            | "rcx"
            | "dl"
            | "dh"
            | "dx"
            | "edx"
            | "rdx"
            | "si"
            | "esi"
            | "rsi"
            | "di"
            | "edi"
            | "rdi"
            | "sp"
            | "esp"
            | "rsp"
            | "bp"
            | "ebp"
            | "rbp"
            | "ip"
            | "eip"
            | "rip"
            | "spl"
            | "bpl"
            | "sil"
            | "dil"
            | "cs"
            | "ds"
            | "es"
            | "fs"
            | "gs"
            | "ss"
            | "st"
    ) || numbered_register(&token, "r", 15, &["", "b", "w", "d"])
        || numbered_register(&token, "xmm", 31, &[""])
        || numbered_register(&token, "ymm", 31, &[""])
        || numbered_register(&token, "zmm", 31, &[""])
        || numbered_register(&token, "mm", 7, &[""])
        || numbered_register(&token, "k", 7, &[""])
        || numbered_register(&token, "cr", 15, &[""])
        || numbered_register(&token, "dr", 15, &[""])
        || token
            .strip_prefix("st(")
            .and_then(|suffix| suffix.strip_suffix(')'))
            .and_then(|digits| digits.parse::<u8>().ok())
            .is_some_and(|index| index <= 7)
}

fn is_aarch64_register(token: &str) -> bool {
    let token = token.to_ascii_lowercase();
    matches!(
        token.as_str(),
        "sp" | "wsp" | "xzr" | "wzr" | "lr" | "fp" | "ip0" | "ip1"
    ) || numbered_register(&token, "x", 31, &[""])
        || numbered_register(&token, "w", 31, &[""])
        || numbered_register(&token, "q", 31, &[""])
        || numbered_register(&token, "d", 31, &[""])
        || numbered_register(&token, "s", 31, &[""])
        || numbered_register(&token, "h", 31, &[""])
        || numbered_register(&token, "b", 31, &[""])
        || numbered_register(&token, "v", 31, &[""])
}

fn numbered_register(token: &str, prefix: &str, max: u8, suffixes: &[&str]) -> bool {
    let Some(rest) = token.strip_prefix(prefix) else {
        return false;
    };
    let digits_len = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits_len == 0 {
        return false;
    }
    let (digits, suffix) = rest.split_at(digits_len);
    digits
        .parse::<u8>()
        .ok()
        .is_some_and(|value| value <= max && suffixes.contains(&suffix))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::{resolve_patch_symbols, ResolvedPatchSymbol};
    use crate::executable::types::SymbolInfo;
    use crate::executable::{
        Bitness, Endian, ExecutableArch, ExecutableInfo, ExecutableKind, PatchSymbolLookup,
        SymbolSource, SymbolType,
    };

    fn info() -> ExecutableInfo {
        let mut symbols_by_va = BTreeMap::new();
        symbols_by_va.insert(
            0x401000,
            SymbolInfo {
                display_name: "entry".to_owned(),
                raw_name: Some("_entry".to_owned()),
                source: SymbolSource::Object,
                size: 0,
                symbol_type: SymbolType::Function,
            },
        );
        let mut symbols_by_name = HashMap::new();
        symbols_by_name.insert("entry".to_owned(), vec![0x401000]);
        let mut target_names_by_va = BTreeMap::new();
        target_names_by_va.insert(0x401030, "strcmp".to_owned());
        let mut target_names_by_name = HashMap::new();
        target_names_by_name.insert("strcmp".to_owned(), vec![0x401030]);
        ExecutableInfo {
            kind: ExecutableKind::Elf,
            arch: ExecutableArch::X86_64,
            bitness: Bitness::Bit64,
            endian: Endian::Little,
            entry_offset: None,
            entry_virtual_address: None,
            code_spans: Vec::new(),
            symbols_by_va,
            target_names_by_va: Box::new(target_names_by_va),
            symbols_by_name,
            target_names_by_name,
            imports: Vec::new(),
        }
    }

    #[test]
    fn resolves_direct_branch_symbol_names() {
        let resolved = resolve_patch_symbols(&info(), ExecutableArch::X86_64, "call strcmp")
            .expect("resolved");
        assert_eq!(resolved.statement, "call 0x401030");
        assert_eq!(
            resolved.resolved_symbol,
            Some(ResolvedPatchSymbol {
                token: "strcmp".to_owned(),
                address: 0x401030,
            })
        );
    }

    #[test]
    fn leaves_indirect_register_calls_unchanged() {
        let resolved =
            resolve_patch_symbols(&info(), ExecutableArch::X86_64, "call rax").expect("resolved");
        assert_eq!(resolved.statement, "call rax");
        assert!(resolved.resolved_symbol.is_none());
    }

    #[test]
    fn rejects_symbol_operands_outside_direct_branch_calls() {
        let err = resolve_patch_symbols(&info(), ExecutableArch::X86_64, "mov eax, strcmp")
            .expect_err("unsupported");
        assert!(err
            .to_string()
            .contains("symbol patch only supports direct branch/call operands"));
    }

    #[test]
    fn executable_lookup_reports_ambiguous_names() {
        let mut info = info();
        info.symbols_by_name
            .insert("dup".to_owned(), vec![0x401000, 0x401010]);
        assert_eq!(
            info.lookup_patch_symbol("dup"),
            PatchSymbolLookup::Ambiguous(vec![0x401000, 0x401010])
        );
    }
}
