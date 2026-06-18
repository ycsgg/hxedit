use super::*;

// ═══════════════════════════════════════════════════════════════════════════
// Hash command: various algorithms and ranges
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hash_command_various_algorithms_and_ranges() {
    // SHA256 on entire file
    let mut app = app_with_bytes(b"hello");
    app.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app.status_message.contains("sha256"));
    assert!(app.status_message.contains("entire file"));
    assert!(app.status_message.contains("2cf24dba5fb0a30e"));

    // CRC32
    let mut app2 = app_with_bytes(b"hello");
    app2.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Crc32,
    })
    .unwrap();
    assert!(app2.status_message.contains("crc32"));

    // Visual selection
    let mut app3 = app_with_bytes(b"hello world");
    app3.toggle_visual();
    app3.move_horizontal(4);
    app3.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Md5,
    })
    .unwrap();
    assert!(app3.status_message.contains("md5"));
    assert!(app3.status_message.contains("sel 0x"));

    // Inspector field
    let mut app4 = app_with_inspector_field(b"hello world", 6, 5);
    app4.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app4.status_message.contains("sel 0x6-0xa"));
    assert!(app4.status_message.contains("486ea46224d1bb4f"));

    // Empty file
    let dir = tempdir().unwrap();
    let file = dir.path().join("empty.bin");
    fs::write(&file, []).unwrap();
    let cli = Cli {
        file: Some(file),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: false,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
    };
    let mut app5 = App::from_cli(cli).unwrap();
    app5.execute_command(Command::Hash {
        algorithm: HashAlgorithm::Sha256,
    })
    .unwrap();
    assert!(app5.status_message.contains("no data"));
}
