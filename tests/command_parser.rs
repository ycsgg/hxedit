use hxedit::commands::parser::parse_command;
use std::path::PathBuf;

use hxedit::commands::types::{Command, ExportFormat, GotoTarget, HashAlgorithm};
use hxedit::copy::{CopyDisplay, CopyFormat};

#[test]
fn parses_basic_commands() {
    assert_eq!(parse_command("q").unwrap(), Command::Quit { force: false });
    assert_eq!(parse_command("q!").unwrap(), Command::Quit { force: true });
    assert_eq!(
        parse_command("wq").unwrap(),
        Command::WriteQuit { path: None }
    );
    assert_eq!(
        parse_command("goto 0x20").unwrap(),
        Command::Goto {
            target: GotoTarget::Absolute(0x20)
        }
    );
    assert_eq!(
        parse_command("goto end").unwrap(),
        Command::Goto {
            target: GotoTarget::End
        }
    );
    assert_eq!(
        parse_command("goto +16").unwrap(),
        Command::Goto {
            target: GotoTarget::Relative(16)
        }
    );
    assert_eq!(
        parse_command("goto -0x10").unwrap(),
        Command::Goto {
            target: GotoTarget::Relative(-0x10)
        }
    );
    assert_eq!(parse_command("u").unwrap(), Command::Undo { steps: 1 });
    assert_eq!(parse_command("undo 3").unwrap(), Command::Undo { steps: 3 });
    assert_eq!(parse_command("redo").unwrap(), Command::Redo { steps: 1 });
    assert_eq!(parse_command("redo 2").unwrap(), Command::Redo { steps: 2 });
    assert_eq!(
        parse_command("p").unwrap(),
        Command::Paste {
            raw: false,
            preview: false,
            limit: None,
        }
    );
    assert_eq!(
        parse_command("paste! 8").unwrap(),
        Command::Paste {
            raw: true,
            preview: false,
            limit: Some(8),
        }
    );
    assert_eq!(
        parse_command("p? 4").unwrap(),
        Command::Paste {
            raw: false,
            preview: true,
            limit: Some(4),
        }
    );
    assert_eq!(
        parse_command("p!? 2").unwrap(),
        Command::Paste {
            raw: true,
            preview: true,
            limit: Some(2),
        }
    );
    assert_eq!(
        parse_command("copy").unwrap(),
        Command::Copy {
            format: CopyFormat::Byte,
            display: CopyDisplay::Raw,
        }
    );
    assert_eq!(
        parse_command("c db nl").unwrap(),
        Command::Copy {
            format: CopyFormat::DoubleByte,
            display: CopyDisplay::NumericLittle,
        }
    );
    assert_eq!(
        parse_command("copy b64").unwrap(),
        Command::Copy {
            format: CopyFormat::Byte,
            display: CopyDisplay::Base64,
        }
    );
    assert_eq!(
        parse_command("fill de ad 8").unwrap(),
        Command::Fill {
            pattern: vec![0xde, 0xad],
            len: 8,
        }
    );
    assert_eq!(
        parse_command("zero 16").unwrap(),
        Command::Fill {
            pattern: vec![0x00],
            len: 16,
        }
    );
    assert_eq!(
        parse_command("export /tmp/out.bin").unwrap(),
        Command::Export {
            format: ExportFormat::Binary {
                path: PathBuf::from("/tmp/out.bin")
            }
        }
    );
    assert_eq!(
        parse_command("export c payload").unwrap(),
        Command::Export {
            format: ExportFormat::CArray {
                name: "payload".to_owned()
            }
        }
    );
    assert_eq!(
        parse_command("export py blob").unwrap(),
        Command::Export {
            format: ExportFormat::PythonBytes {
                name: "blob".to_owned()
            }
        }
    );
    assert_eq!(
        parse_command("re de ad -> be ef").unwrap(),
        Command::Replace {
            needle: vec![0xde, 0xad],
            replacement: vec![0xbe, 0xef],
            allow_resize: false,
        }
    );
    assert_eq!(
        parse_command("replace ascii hello -> world").unwrap(),
        Command::Replace {
            needle: b"hello".to_vec(),
            replacement: b"world".to_vec(),
            allow_resize: false,
        }
    );
    assert_eq!(
        parse_command("re! ascii hello -> hi").unwrap(),
        Command::Replace {
            needle: b"hello".to_vec(),
            replacement: b"hi".to_vec(),
            allow_resize: true,
        }
    );
    assert_eq!(
        parse_command("s hello").unwrap(),
        Command::SearchAscii {
            pattern: b"hello".to_vec(),
            backward: false,
        }
    );
    assert_eq!(
        parse_command("s! hello").unwrap(),
        Command::SearchAscii {
            pattern: b"hello".to_vec(),
            backward: true,
        }
    );
    assert_eq!(
        parse_command("S 7f 45 4c 46").unwrap(),
        Command::SearchHex {
            pattern: vec![0x7f, 0x45, 0x4c, 0x46],
            backward: false,
        }
    );
    assert_eq!(
        parse_command("S! 7f 45 4c 46").unwrap(),
        Command::SearchHex {
            pattern: vec![0x7f, 0x45, 0x4c, 0x46],
            backward: true,
        }
    );
    #[cfg(feature = "disasm")]
    assert_eq!(
        parse_command("si mov rax").unwrap(),
        Command::SearchInstruction {
            pattern: "mov rax".to_owned(),
            backward: false,
        }
    );
    #[cfg(feature = "disasm")]
    assert_eq!(
        parse_command("si! ret").unwrap(),
        Command::SearchInstruction {
            pattern: "ret".to_owned(),
            backward: true,
        }
    );
    #[cfg(not(feature = "disasm"))]
    assert!(parse_command("si mov rax").is_err());
    #[cfg(not(feature = "disasm"))]
    assert!(parse_command("si! ret").is_err());
    assert_eq!(parse_command("data").unwrap(), Command::Data);
    assert_eq!(parse_command("data off").unwrap(), Command::DataOff);
}

#[test]
fn rejects_invalid_commands() {
    assert!(parse_command("goto nope").is_err());
    assert!(parse_command("fill ff 0").is_err());
    assert!(parse_command("fill 8").is_err());
    assert!(parse_command("zero nope").is_err());
    assert!(parse_command("re de ad -> be").is_err());
    assert!(parse_command("re!").is_err());
    assert!(parse_command("re ascii hello world").is_err());
    assert!(parse_command("undo nope").is_err());
    assert!(parse_command("undo 0").is_err());
    assert!(parse_command("redo nope").is_err());
    assert!(parse_command("redo 0").is_err());
    assert!(parse_command("paste nope").is_err());
    assert!(parse_command("export").is_err());
    assert!(parse_command("copy nope").is_err());
    assert!(parse_command("S 0xz1").is_err());
    assert!(parse_command("si").is_err());
    assert!(parse_command("hash").is_err());
    assert!(parse_command("hash blake2").is_err());
    assert!(parse_command("dis!").is_err());
    assert!(parse_command("dis! x86_64").is_err());
    assert!(parse_command("data more").is_err());
    assert!(parse_command("unknown").is_err());
}

#[test]
fn parses_uppercase_hex_search_patterns() {
    assert_eq!(
        parse_command("S DE AD BE EF").unwrap(),
        Command::SearchHex {
            pattern: vec![0xde, 0xad, 0xbe, 0xef],
            backward: false,
        }
    );
    assert_eq!(
        parse_command("S 7F 45 4C 46").unwrap(),
        Command::SearchHex {
            pattern: vec![0x7f, 0x45, 0x4c, 0x46],
            backward: false,
        }
    );
}

#[test]
fn parses_hash_commands() {
    assert_eq!(
        parse_command("hash md5").unwrap(),
        Command::Hash {
            algorithm: HashAlgorithm::Md5
        }
    );
    assert_eq!(
        parse_command("hash sha256").unwrap(),
        Command::Hash {
            algorithm: HashAlgorithm::Sha256
        }
    );
    assert_eq!(
        parse_command("hash crc32").unwrap(),
        Command::Hash {
            algorithm: HashAlgorithm::Crc32
        }
    );
}

#[cfg(feature = "disasm")]
#[test]
fn parses_disassembly_commands() {
    assert_eq!(
        parse_command("dis").unwrap(),
        Command::Disassemble { arch: None }
    );
    assert_eq!(
        parse_command("dis x86_64").unwrap(),
        Command::Disassemble {
            arch: Some("x86_64".to_owned())
        }
    );
    assert_eq!(
        parse_command("dis! x86_64 0x20").unwrap(),
        Command::DisassembleForce {
            arch: "x86_64".to_owned(),
            offset: 0x20,
        }
    );
    assert_eq!(parse_command("dis off").unwrap(), Command::DisassembleOff);
}

#[cfg(not(feature = "disasm"))]
#[test]
fn disassembly_commands_are_hidden_without_feature() {
    assert!(parse_command("dis").is_err());
    assert!(parse_command("dis x86_64").is_err());
    assert!(parse_command("dis! x86_64 0x20").is_err());
}
