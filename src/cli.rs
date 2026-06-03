use std::path::PathBuf;

use clap::Parser;

use crate::config::Config;
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

    /// Open with the side panel visible on the inspector page
    #[arg(long)]
    pub inspector: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliTarget {
    File(PathBuf),
    Pid(u32),
    Process(String),
}

impl Cli {
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
        Ok(Config {
            bytes_per_line: self.bytes_per_line.max(1),
            page_size: self.page_size.max(256),
            cache_pages: self.cache_pages.max(4),
            profile: self.profile,
            readonly: self.readonly,
            color_level: ColorLevel::detect(self.no_color),
            initial_offset: match &self.offset {
                Some(value) => parse_offset(value)?,
                None => 0,
            },
            inspector: self.inspector,
        })
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
            bytes_per_line: 16,
            page_size: 4096,
            cache_pages: 8,
            profile: false,
            readonly: false,
            no_color: true,
            offset: None,
            inspector: false,
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
