use std::path::PathBuf;

use clap::Parser;

use crate::config::{Config, FileConfig};
use crate::error::{HxError, HxResult};
use crate::util::parse::parse_offset;

use crate::view::palette::ColorLevel;

#[derive(Debug, Parser)]
#[command(name = "hxedit", version, about = "Hex editor for the terminal")]
pub struct Cli {
    #[arg(value_name = "FILE")]
    pub file: Option<PathBuf>,

    #[arg(long, value_name = "PID")]
    pub pid: Option<u32>,

    #[arg(long, value_name = "NAME")]
    pub process: Option<String>,

    #[arg(long, value_name = "PATH", help = "Path to a config file (TOML)")]
    pub config: Option<PathBuf>,

    #[arg(long, value_name = "N")]
    pub bytes_per_line: Option<usize>,

    #[arg(long, value_name = "N")]
    pub page_size: Option<usize>,

    #[arg(long, value_name = "N")]
    pub cache_pages: Option<usize>,

    #[arg(long, help = "Print startup and render diagnostics to stderr")]
    pub profile: bool,

    #[arg(long)]
    pub readonly: bool,

    #[arg(long)]
    pub no_color: bool,

    #[arg(long, value_name = "OFFSET")]
    pub offset: Option<String>,

    /// Open with the side panel visible on the inspector page
    #[arg(long)]
    pub inspector: bool,

    /// Run a TOML macro file headlessly and exit
    #[arg(long = "run", value_name = "PATH")]
    pub run: Vec<PathBuf>,

    /// Run an editor command headlessly and exit; may be repeated
    #[arg(long = "command", value_name = "COMMAND")]
    pub command: Vec<String>,

    /// Initial headless selection, e.g. display:0x100:16 or logical:0:32
    #[arg(long = "select", value_name = "SPACE:START:LEN")]
    pub select: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliTarget {
    File(PathBuf),
    Pid(u32),
    Process(String),
}

impl Cli {
    pub fn has_headless_actions(&self) -> bool {
        !self.run.is_empty() || !self.command.is_empty()
    }

    pub fn target(&self) -> HxResult<CliTarget> {
        let source_count = usize::from(self.file.is_some())
            + usize::from(self.pid.is_some())
            + usize::from(self.process.is_some());
        match source_count {
            0 => Err(HxError::InvalidCliSource(
                "a file or a memory target is required".to_owned(),
            )),
            1 => {
                if let Some(file) = &self.file {
                    Ok(CliTarget::File(file.clone()))
                } else if let Some(pid) = self.pid {
                    Ok(CliTarget::Pid(pid))
                } else {
                    Ok(CliTarget::Process(
                        self.process.clone().expect("process was counted"),
                    ))
                }
            }
            _ => Err(HxError::InvalidCliSource(
                "file, --pid, and --process are mutually exclusive".to_owned(),
            )),
        }
    }

    pub fn config(&self) -> anyhow::Result<Config> {
        // Priority: defaults < config file < explicit CLI flags.
        let mut config = Config::default();

        if let Some(path) = crate::config::resolve_config_path(self.config.as_deref()) {
            FileConfig::load(&path)?.apply_to(&mut config);
        }

        if let Some(value) = self.bytes_per_line {
            config.bytes_per_line = value;
        }
        if let Some(value) = self.page_size {
            config.page_size = value;
        }
        if let Some(value) = self.cache_pages {
            config.cache_pages = value;
        }
        if self.profile {
            config.profile = true;
        }
        if self.readonly {
            config.readonly = true;
        }
        if self.no_color {
            config.color_level = ColorLevel::NoColor;
        }
        if self.inspector {
            config.inspector = true;
        }
        if let Some(value) = &self.offset {
            config.initial_offset = parse_offset(value)?;
        }

        // Clamp to safe lower bounds regardless of where the value came from.
        config.bytes_per_line = config.bytes_per_line.max(1);
        config.page_size = config.page_size.max(256);
        config.cache_pages = config.cache_pages.max(4);
        config.data_panel_bytes = config.data_panel_bytes.max(1);
        config.export_c_width = config.export_c_width.max(1);
        config.export_py_width = config.export_py_width.max(1);

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, CliTarget};

    fn base_cli() -> Cli {
        Cli {
            file: None,
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
        }
    }

    #[test]
    fn target_requires_exactly_one_source() {
        assert!(base_cli().target().is_err());

        let mut cli = base_cli();
        cli.file = Some("test.bin".into());
        cli.pid = Some(123);
        assert!(cli.target().is_err());
    }

    #[test]
    fn target_accepts_file_pid_or_process() {
        let mut cli = base_cli();
        cli.file = Some("test.bin".into());
        assert!(matches!(cli.target().unwrap(), CliTarget::File(_)));

        let mut cli = base_cli();
        cli.pid = Some(123);
        assert_eq!(cli.target().unwrap(), CliTarget::Pid(123));

        let mut cli = base_cli();
        cli.process = Some("target".to_owned());
        assert_eq!(
            cli.target().unwrap(),
            CliTarget::Process("target".to_owned())
        );
    }
}
