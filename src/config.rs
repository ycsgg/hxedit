#[derive(Debug, Clone)]
pub struct Config {
    pub bytes_per_line: usize,
    pub page_size: usize,
    pub cache_pages: usize,
    pub readonly: bool,
    pub color: bool,
    pub initial_offset: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bytes_per_line: 16,
            page_size: 16 * 1024,
            cache_pages: 128,
            readonly: false,
            color: true,
            initial_offset: 0,
        }
    }
}
