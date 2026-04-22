#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionTextTokenKind {
    Whitespace,
    Punctuation,
    Atom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstructionTextToken<'a> {
    pub kind: InstructionTextTokenKind,
    pub text: &'a str,
}

pub fn tokenize_instruction_text(text: &str) -> Vec<InstructionTextToken<'_>> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        let kind = if ch.is_whitespace() {
            InstructionTextTokenKind::Whitespace
        } else if is_punctuation(ch) {
            InstructionTextTokenKind::Punctuation
        } else {
            InstructionTextTokenKind::Atom
        };
        let mut end = start + ch.len_utf8();

        while let Some(&(idx, next)) = chars.peek() {
            let same_kind = match kind {
                InstructionTextTokenKind::Whitespace => next.is_whitespace(),
                InstructionTextTokenKind::Punctuation => false,
                InstructionTextTokenKind::Atom => !next.is_whitespace() && !is_punctuation(next),
            };
            if !same_kind {
                break;
            }
            chars.next();
            end = idx + next.len_utf8();
        }

        tokens.push(InstructionTextToken {
            kind,
            text: &text[start..end],
        });
    }

    tokens
}

pub fn looks_like_register(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| ch == '%' || ch == '#');
    let lower = token.to_ascii_lowercase();
    matches!(
        lower.as_str(),
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
            | "bp"
            | "ebp"
            | "rbp"
            | "sp"
            | "esp"
            | "rsp"
            | "ip"
            | "eip"
            | "rip"
            | "pc"
            | "lr"
            | "fp"
            | "xzr"
            | "wzr"
            | "nzcv"
            | "cpsr"
            | "spsr"
    ) || lower.starts_with('r') && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with('x') && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with('w') && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("v") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("q") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("d") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("s") && lower[1..].chars().all(|ch| ch.is_ascii_digit())
        || lower.starts_with("zmm")
        || lower.starts_with("ymm")
        || lower.starts_with("xmm")
}

pub fn looks_like_immediate(token: &str) -> bool {
    parse_immediate_token(token).is_some()
}

pub fn parse_immediate_token(token: &str) -> Option<u64> {
    let trimmed = token.trim();
    let trimmed = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix('-').unwrap_or(trimmed);
    if let Some(hex) = trimmed.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some(hex) = trimmed.strip_suffix('h') {
        return (!hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit()))
            .then(|| u64::from_str_radix(hex, 16).ok())
            .flatten();
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_digit())
        .then(|| trimmed.parse().ok())
        .flatten()
}

fn is_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ',' | '[' | ']' | '(' | ')' | '{' | '}' | '+' | '-' | '*' | ':' | '!' | '='
    )
}

#[cfg(test)]
mod tests {
    use super::{
        looks_like_immediate, looks_like_register, parse_immediate_token,
        tokenize_instruction_text, InstructionTextTokenKind,
    };

    #[test]
    fn tokenizer_splits_operands_by_shared_rules() {
        let tokens = tokenize_instruction_text("rax, [rbx+0x10]");
        let kinds = tokens.iter().map(|token| token.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                InstructionTextTokenKind::Atom,
                InstructionTextTokenKind::Punctuation,
                InstructionTextTokenKind::Whitespace,
                InstructionTextTokenKind::Punctuation,
                InstructionTextTokenKind::Atom,
                InstructionTextTokenKind::Punctuation,
                InstructionTextTokenKind::Atom,
                InstructionTextTokenKind::Punctuation,
            ]
        );
    }

    #[test]
    fn immediate_parser_accepts_hex_decimal_and_suffix_h() {
        assert_eq!(parse_immediate_token("0x10"), Some(0x10));
        assert_eq!(parse_immediate_token("16"), Some(16));
        assert_eq!(parse_immediate_token("10h"), Some(0x10));
        assert!(looks_like_immediate("#0x20"));
        assert!(looks_like_register("x3"));
    }
}
