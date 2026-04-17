const DEFAULT_EXPORT_NAME: &str = "selection_bytes";
const BYTES_PER_C_LINE: usize = 12;
const BYTES_PER_PY_CHUNK: usize = 16;

pub fn sanitize_identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len().max(DEFAULT_EXPORT_NAME.len()));

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    if out.is_empty() {
        return DEFAULT_EXPORT_NAME.to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

pub fn format_c_array(name: &str, bytes: &[u8]) -> String {
    let ident = sanitize_identifier(name);
    let mut out = String::new();
    out.push_str(&format!("static const unsigned char {ident}[] = {{\n"));

    if bytes.is_empty() {
        out.push_str("};\n");
    } else {
        for chunk in bytes.chunks(BYTES_PER_C_LINE) {
            out.push_str("    ");
            for (idx, byte) in chunk.iter().enumerate() {
                if idx > 0 {
                    out.push(' ');
                }
                out.push_str(&format!("0x{byte:02x},"));
            }
            out.push('\n');
        }
        out.push_str("};\n");
    }

    out.push_str(&format!(
        "static const unsigned int {ident}_len = {};\n",
        bytes.len()
    ));
    out
}

pub fn format_python_bytes(name: &str, bytes: &[u8]) -> String {
    let ident = sanitize_identifier(name);
    if bytes.is_empty() {
        return format!("{ident} = b\"\"\n");
    }

    let chunks = bytes
        .chunks(BYTES_PER_PY_CHUNK)
        .map(|chunk| {
            let body = chunk
                .iter()
                .map(|byte| format!("\\x{byte:02x}"))
                .collect::<String>();
            format!("b\"{body}\"")
        })
        .collect::<Vec<_>>();

    if chunks.len() == 1 {
        format!("{ident} = {}\n", chunks[0])
    } else {
        format!("{ident} = (\n    {}\n)\n", chunks.join("\n    "))
    }
}

#[cfg(test)]
mod tests {
    use super::{format_c_array, format_python_bytes, sanitize_identifier};

    #[test]
    fn sanitize_identifier_rewrites_invalid_chars() {
        assert_eq!(sanitize_identifier("1 bad-name"), "_1_bad_name");
        assert_eq!(sanitize_identifier(""), "selection_bytes");
    }

    #[test]
    fn c_array_export_includes_length() {
        let text = format_c_array("payload", &[0xde, 0xad, 0xbe, 0xef]);
        assert!(text.contains("payload[]"));
        assert!(text.contains("0xde, 0xad, 0xbe, 0xef,"));
        assert!(text.contains("payload_len = 4;"));
    }

    #[test]
    fn python_bytes_export_uses_hex_escapes() {
        let text = format_python_bytes("payload", &[0x00, 0xff]);
        assert_eq!(text, "payload = b\"\\x00\\xff\"\n");
    }
}
