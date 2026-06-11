use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::view::palette::ColorLevel;

/// Runtime configuration resolved from defaults, the config file, and CLI
/// overrides (in increasing priority).
#[derive(Debug, Clone)]
pub struct Config {
    pub bytes_per_line: usize,
    pub page_size: usize,
    pub cache_pages: usize,
    pub profile: bool,
    pub readonly: bool,
    pub color_level: ColorLevel,
    pub initial_offset: u64,
    pub inspector: bool,
    pub data_panel_bytes: usize,
    pub inspector_depth: usize,
    pub export_c_width: usize,
    pub export_py_width: usize,
    pub export_name: String,
    pub search_wrap: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bytes_per_line: 16,
            page_size: 16 * 1024,
            cache_pages: 128,
            profile: false,
            readonly: false,
            color_level: ColorLevel::detect(false),
            initial_offset: 0,
            inspector: false,
            data_panel_bytes: 16,
            inspector_depth: 1,
            export_c_width: 12,
            export_py_width: 16,
            export_name: "selection_bytes".to_owned(),
            search_wrap: true,
        }
    }
}

/// How the config file wants colors to be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorChoice {
    /// Auto-detect terminal capabilities (default).
    Auto,
    /// Force colors off, like `--no-color`.
    Never,
}

/// On-disk config file (`config.toml`). Every field is optional so an empty or
/// partial file still parses; missing values fall back to [`Config::default`].
///
/// Sections mirror the user-facing TOML layout: `[display]`, `[behavior]`,
/// `[performance]`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub behavior: BehaviorConfig,
    #[serde(default)]
    pub performance: PerformanceConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayConfig {
    pub bytes_per_line: Option<usize>,
    pub data_panel_bytes: Option<usize>,
    pub inspector_depth: Option<usize>,
    pub export_c_width: Option<usize>,
    pub export_py_width: Option<usize>,
    pub export_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BehaviorConfig {
    pub readonly: Option<bool>,
    pub inspector: Option<bool>,
    pub color: Option<ColorChoice>,
    pub search_wrap: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceConfig {
    pub page_size: Option<usize>,
    pub cache_pages: Option<usize>,
}

impl FileConfig {
    /// Parse a `FileConfig` from TOML text.
    pub fn from_toml(text: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Load the config file from `path`. Returns the default config when the
    /// file does not exist; surfaces an error when it exists but cannot be
    /// read or parsed.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text)
                .map_err(|err| anyhow::anyhow!("failed to parse {}: {err}", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(anyhow::anyhow!("failed to read {}: {err}", path.display())),
        }
    }

    /// Apply file values onto `config`, overriding defaults where present.
    pub fn apply_to(&self, config: &mut Config) {
        if let Some(value) = self.display.bytes_per_line {
            config.bytes_per_line = value;
        }
        if let Some(value) = self.display.data_panel_bytes {
            config.data_panel_bytes = value;
        }
        if let Some(value) = self.display.inspector_depth {
            config.inspector_depth = value;
        }
        if let Some(value) = self.display.export_c_width {
            config.export_c_width = value;
        }
        if let Some(value) = self.display.export_py_width {
            config.export_py_width = value;
        }
        if let Some(value) = &self.display.export_name {
            config.export_name = value.clone();
        }
        if let Some(value) = self.behavior.readonly {
            config.readonly = value;
        }
        if let Some(value) = self.behavior.inspector {
            config.inspector = value;
        }
        if let Some(ColorChoice::Never) = self.behavior.color {
            config.color_level = ColorLevel::NoColor;
        }
        if let Some(value) = self.behavior.search_wrap {
            config.search_wrap = value;
        }
        if let Some(value) = self.performance.page_size {
            config.page_size = value;
        }
        if let Some(value) = self.performance.cache_pages {
            config.cache_pages = value;
        }
    }
}

/// Resolve which config file path to load.
///
/// Priority: explicit `--config` path > `$HXEDIT_CONFIG` > the platform config
/// directory (`~/.config/hxedit/config.toml` on Linux/macOS). Returns `None`
/// when no path can be determined (e.g. no home directory).
pub fn resolve_config_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if let Some(value) = std::env::var_os("HXEDIT_CONFIG") {
        return Some(PathBuf::from(value));
    }
    directories::ProjectDirs::from("", "", "hxedit")
        .map(|dirs| dirs.config_dir().join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_keeps_defaults() {
        let file = FileConfig::from_toml("").unwrap();
        let mut config = Config::default();
        file.apply_to(&mut config);
        assert_eq!(config.bytes_per_line, 16);
        assert_eq!(config.page_size, 16 * 1024);
        assert_eq!(config.cache_pages, 128);
        assert!(config.search_wrap);
        assert_eq!(config.export_name, "selection_bytes");
    }

    #[test]
    fn full_toml_overrides_defaults() {
        let text = r#"
[display]
bytes_per_line = 32
data_panel_bytes = 24
inspector_depth = 2
export_c_width = 8
export_py_width = 20
export_name = "payload"

[behavior]
readonly = true
inspector = true
color = "never"
search_wrap = false

[performance]
page_size = 8192
cache_pages = 64
"#;
        let file = FileConfig::from_toml(text).unwrap();
        let mut config = Config::default();
        file.apply_to(&mut config);
        assert_eq!(config.bytes_per_line, 32);
        assert_eq!(config.data_panel_bytes, 24);
        assert_eq!(config.inspector_depth, 2);
        assert_eq!(config.export_c_width, 8);
        assert_eq!(config.export_py_width, 20);
        assert_eq!(config.export_name, "payload");
        assert!(config.readonly);
        assert!(config.inspector);
        assert_eq!(config.color_level, ColorLevel::NoColor);
        assert!(!config.search_wrap);
        assert_eq!(config.page_size, 8192);
        assert_eq!(config.cache_pages, 64);
    }

    #[test]
    fn partial_section_only_overrides_present_keys() {
        let file = FileConfig::from_toml("[behavior]\nsearch_wrap = false\n").unwrap();
        let mut config = Config::default();
        file.apply_to(&mut config);
        assert!(!config.search_wrap);
        // untouched fields keep their defaults
        assert_eq!(config.bytes_per_line, 16);
        assert!(!config.readonly);
    }

    #[test]
    fn unknown_field_is_rejected() {
        assert!(FileConfig::from_toml("[display]\nbogus = 1\n").is_err());
        assert!(FileConfig::from_toml("[bogus]\nx = 1\n").is_err());
    }

    #[test]
    fn invalid_value_type_is_rejected() {
        assert!(FileConfig::from_toml("[display]\nbytes_per_line = \"x\"\n").is_err());
    }

    #[test]
    fn missing_file_yields_defaults() {
        let path = std::path::Path::new("/nonexistent/hxedit/does-not-exist.toml");
        let file = FileConfig::load(path).unwrap();
        let mut config = Config::default();
        file.apply_to(&mut config);
        assert_eq!(config.bytes_per_line, 16);
    }

    #[test]
    fn explicit_path_takes_priority() {
        let explicit = std::path::Path::new("/tmp/custom.toml");
        assert_eq!(
            resolve_config_path(Some(explicit)),
            Some(explicit.to_path_buf())
        );
    }
}
