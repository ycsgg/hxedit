use std::path::Path;

use hxedit::app::App;
use hxedit::cli::Cli;

#[test]
fn app_constructs_from_cli() {
    let cli = Cli {
        file: Path::new("tests/fixtures/ascii.bin").to_path_buf(),
        bytes_per_line: 16,
        page_size: 4096,
        cache_pages: 8,
        readonly: true,
        no_color: true,
        offset: None,
    };
    App::from_cli(cli).unwrap();
}
