use anyhow::Result;
use clap::Parser;

use hxedit::app::App;
use hxedit::cli::Cli;
use hxedit::error::HxError;

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.has_headless_actions() {
        return hxedit::headless::run(cli);
    }
    if cli.select.is_some() {
        return Err(
            HxError::InvalidCliSource("--select requires --run or --command".to_owned()).into(),
        );
    }
    let mut app = App::from_cli(cli)?;
    app.run()
}
