use hxedit::commands::parser::parse_command;
use hxedit::commands::types::Command;
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
        Command::Goto { offset: 0x20 }
    );
    assert_eq!(parse_command("u").unwrap(), Command::Undo { steps: 1 });
    assert_eq!(parse_command("undo 3").unwrap(), Command::Undo { steps: 3 });
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
}

#[test]
fn rejects_invalid_commands() {
    assert!(parse_command("goto nope").is_err());
    assert!(parse_command("undo nope").is_err());
    assert!(parse_command("undo 0").is_err());
    assert!(parse_command("paste nope").is_err());
    assert!(parse_command("copy nope").is_err());
    assert!(parse_command("S 0xz1").is_err());
    assert!(parse_command("unknown").is_err());
}
