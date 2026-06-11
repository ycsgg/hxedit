pub fn sanitize_identifier(name: &str, default_name: &str) -> String {
    let mut out = String::with_capacity(name.len().max(default_name.len()));

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    if out.is_empty() {
        return default_name.to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

pub fn format_c_array(name: &str, bytes: &[u8], bytes_per_line: usize) -> String {
    let ident = name;
    let mut out = String::new();
    out.push_str(&format!("static const unsigned char {ident}[] = {{\n"));

    if bytes.is_empty() {
        out.push_str("};\n");
    } else {
        for chunk in bytes.chunks(bytes_per_line.max(1)) {
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

pub fn format_python_bytes(name: &str, bytes: &[u8], bytes_per_chunk: usize) -> String {
    let ident = name;
    if bytes.is_empty() {
        return format!("{ident} = b\"\"\n");
    }

    let chunks = bytes
        .chunks(bytes_per_chunk.max(1))
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
        assert_eq!(
            sanitize_identifier("1 bad-name", "selection_bytes"),
            "_1_bad_name"
        );
        assert_eq!(
            sanitize_identifier("", "selection_bytes"),
            "selection_bytes"
        );
    }

    #[test]
    fn c_array_export_includes_length() {
        let text = format_c_array("payload", &[0xde, 0xad, 0xbe, 0xef], 12);
        assert!(text.contains("payload[]"));
        assert!(text.contains("0xde, 0xad, 0xbe, 0xef,"));
        assert!(text.contains("payload_len = 4;"));
    }

    #[test]
    fn python_bytes_export_uses_hex_escapes() {
        let text = format_python_bytes("payload", &[0x00, 0xff], 16);
        assert_eq!(text, "payload = b\"\\x00\\xff\"\n");
    }
}
