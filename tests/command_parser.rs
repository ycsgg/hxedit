use hxedit::commands::parser::parse_command;
use hxedit::commands::types::Command;

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
    assert_eq!(
        parse_command("s hello").unwrap(),
        Command::SearchAscii {
            pattern: b"hello".to_vec(),
        }
    );
    assert_eq!(
        parse_command("S 7f 45 4c 46").unwrap(),
        Command::SearchHex {
            pattern: vec![0x7f, 0x45, 0x4c, 0x46],
        }
    );
}

#[test]
fn rejects_invalid_commands() {
    assert!(parse_command("goto nope").is_err());
    assert!(parse_command("S 0xz1").is_err());
    assert!(parse_command("unknown").is_err());
}
