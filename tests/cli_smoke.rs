use std::path::Path;

use hxedit::app::App;
use hxedit::cli::Cli;

#[test]
fn app_constructs_from_cli() {
    let cli = Cli {
        file: Some(Path::new("tests/fixtures/ascii.bin").to_path_buf()),
        pid: None,
        process: None,
        config: None,
        bytes_per_line: Some(16),
        page_size: Some(4096),
        cache_pages: Some(8),
        profile: false,
        readonly: true,
        no_color: true,
        offset: None,
        inspector: false,
        run: Vec::new(),
        command: Vec::new(),
        select: None,
    };
    App::from_cli(cli).unwrap();
}
