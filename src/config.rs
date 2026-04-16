use crate::view::palette::ColorLevel;

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
        }
    }
}
