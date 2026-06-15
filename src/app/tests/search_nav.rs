use super::*;

// ═══════════════════════════════════════════════════════════════════════════
// Search: forward, backward, wrap
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn search_forward_backward_and_wrap() {
    let mut app = app_with_bytes(b"abc hello xyz hello end");

    // Forward search finds first match
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 4);

    // Search next and prev follow last pattern
    app.repeat_search(SearchDirection::Forward).unwrap();
    assert_eq!(app.cursor, 14);
    app.repeat_search(SearchDirection::Backward).unwrap();
    assert_eq!(app.cursor, 4);

    // Reverse search searches upward
    app.cursor = app.document.len() - 1;
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();
    assert_eq!(app.cursor, 14);
    app.repeat_search(SearchDirection::Backward).unwrap();
    assert_eq!(app.cursor, 4);

    // Forward search wraps to start
    let mut app2 = app_with_bytes(b"hello world");
    app2.cursor = app2.document.len() - 1;
    app2.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app2.cursor, 0);
    assert!(app2.status_message.contains("wrapped"));

    // Backward search wraps to end
    let mut app3 = app_with_bytes(b"hello world hello");
    app3.cursor = 0;
    app3.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();
    assert_eq!(app3.cursor, 12);
    assert!(app3.status_message.contains("wrapped"));
}

#[test]
fn search_wrap_disabled_does_not_wrap() {
    // Forward: cursor past the only match, wrap off -> not found, cursor stays.
    let mut app = app_with_bytes(b"hello world");
    app.config.search_wrap = false;
    app.cursor = app.document.len() - 1;
    app.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, app.document.len() - 1);
    assert!(app.status_message.contains("not found"));
    assert!(app.status_message.contains("wrap off"));

    // Backward: cursor before the only later match, wrap off -> not found.
    let mut app2 = app_with_bytes(b"hello world hello");
    app2.config.search_wrap = false;
    app2.cursor = 0;
    app2.execute_command(Command::SearchAscii {
        pattern: b"hello".to_vec(),
        backward: true,
    })
    .unwrap();
    assert_eq!(app2.cursor, 0);
    assert!(app2.status_message.contains("wrap off"));
}

#[test]
fn unified_search_command_executes_and_warns_for_deprecated_alias() {
    let mut app = app_with_bytes(b"abc hello xyz");
    app.execute_command(crate::commands::parser::parse_command("s /hello/").unwrap())
        .unwrap();
    assert_eq!(app.cursor, 4);
    assert!(app.status_message.contains("found ascii"));

    let mut app2 = app_with_bytes(&[0x00, 0x48, 0x89, 0xc7, 0xff]);
    app2.execute_command(crate::commands::parser::parse_command("s x/48 89 c7/").unwrap())
        .unwrap();
    assert_eq!(app2.cursor, 1);
    assert!(app2.status_message.contains("found hex"));

    let mut app3 = app_with_bytes(&[0x00, 0x48, 0x89, 0xc7, 0xff]);
    app3.execute_command(crate::commands::parser::parse_command("S 48 89 c7").unwrap())
        .unwrap();
    assert_eq!(app3.cursor, 1);
    assert_eq!(app3.status_level, StatusLevel::Warning);
    assert!(app3.status_message.contains("deprecated :S"));
    assert!(app3.status_message.contains(":s x/.../"));
}

#[cfg(all(feature = "disasm", feature = "symbols"))]
#[test]
fn search_repeat_works_while_side_panel_has_focus() {
    let bytes = build_disassembly_elf64_with_symbol(
        &[
            0x90, 0xE8, 0xFA, 0xFF, 0xFF, 0xFF, 0xE8, 0xF5, 0xFF, 0xFF, 0xFF, 0xC3,
        ],
        "entry",
    );
    let mut app = app_with_bytes(&bytes);
    app.execute_command(Command::Disassemble { arch: None })
        .unwrap();
    app.execute_command(Command::Symbols).unwrap();
    assert_eq!(app.mode, Mode::SidePanel);

    app.execute_command(Command::SearchSymbol {
        pattern: "entry".to_owned(),
        backward: false,
    })
    .unwrap();
    assert_eq!(app.cursor, 0x101);

    app.handle_action(Action::SearchNext);
    assert_eq!(app.cursor, 0x106);

    app.handle_action(Action::SearchPrev);
    assert_eq!(app.cursor, 0x101);
}

// ═══════════════════════════════════════════════════════════════════════════
// Goto command: end, relative offsets, delta reporting
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn goto_command_various_targets() {
    let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);

    // End
    app.execute_command(Command::Goto {
        target: GotoTarget::End,
    })
    .unwrap();
    assert_eq!(app.cursor, 3);

    // Relative negative
    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(-2),
    })
    .unwrap();
    assert_eq!(app.cursor, 1);

    // Relative positive
    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(2),
    })
    .unwrap();
    assert_eq!(app.cursor, 3);

    // Delta reporting
    app.cursor = 1;
    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(2),
    })
    .unwrap();
    assert!(app.status_message.contains("moved +0x2"));
    assert!(app.status_message.contains("→ 0x3"));

    app.execute_command(Command::Goto {
        target: GotoTarget::Relative(-1),
    })
    .unwrap();
    assert!(app.status_message.contains("moved -0x1"));
}
