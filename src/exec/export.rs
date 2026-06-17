use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::document::Document;
use crate::error::HxResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryExport {
    pub path: PathBuf,
    pub bytes_written: u64,
}

pub fn export_binary_range(
    document: &mut Document,
    start: u64,
    end_inclusive: u64,
    path: &Path,
) -> HxResult<BinaryExport> {
    let file = std::fs::File::create(path)?;
    let mut writer = std::io::BufWriter::new(file);
    let bytes_written = document.for_each_logical_chunk(start, end_inclusive, |chunk| {
        writer.write_all(chunk)?;
        Ok(())
    })?;
    writer.flush()?;
    Ok(BinaryExport {
        path: path.to_path_buf(),
        bytes_written,
    })
}
