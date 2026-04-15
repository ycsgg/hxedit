use std::path::PathBuf;

use clap::Parser;

use crate::config::Config;
use crate::util::parse::parse_offset;

#[derive(Debug, Parser)]
#[command(name = "hxedit", version, about = "Hex editor for the terminal")]
pub struct Cli {
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    #[arg(long, default_value_t = 16)]
    pub bytes_per_line: usize,

    #[arg(long, default_value_t = 16384)]
    pub page_size: usize,

    #[arg(long, default_value_t = 128)]
    pub cache_pages: usize,

    #[arg(long, help = "Print startup and render diagnostics to stderr")]
    pub profile: bool,

    #[arg(long)]
    pub readonly: bool,

    #[arg(long)]
    pub no_color: bool,

    #[arg(long, value_name = "OFFSET")]
    pub offset: Option<String>,

    /// Open with format inspector panel
    #[arg(long)]
    pub inspector: bool,
}

impl Cli {
    pub fn config(&self) -> anyhow::Result<Config> {
        Ok(Config {
            bytes_per_line: self.bytes_per_line.max(1),
            page_size: self.page_size.max(256),
            cache_pages: self.cache_pages.max(4),
            profile: self.profile,
            readonly: self.readonly,
            color: !self.no_color,
            initial_offset: match &self.offset {
                Some(value) => parse_offset(value)?,
                None => 0,
            },
            inspector: self.inspector,
        })
    }
}
