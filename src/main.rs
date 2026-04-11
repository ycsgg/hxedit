use anyhow::Result;
use clap::Parser;

use hxedit::app::App;
use hxedit::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut app = App::from_cli(cli)?;
    app.run()
}
