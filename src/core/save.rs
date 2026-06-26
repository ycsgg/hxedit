use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::document::walk::WalkControl;
use crate::core::document::Document;
use crate::error::HxResult;

/// Profile information from a save operation.
#[derive(Debug, Clone)]
pub struct SaveProfile {
    pub bytes_written: u64,
    pub pieces: usize,
    pub chunks_read: usize,
    pub fast_chunks: usize,
    pub slow_chunks: usize,
    pub elapsed: Duration,
}

impl fmt::Display for SaveProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mb = self.bytes_written as f64 / (1024.0 * 1024.0);
        let throughput = if self.elapsed.as_secs_f64() > 0.0 {
            mb / self.elapsed.as_secs_f64()
        } else {
            0.0
        };
        write!(
            f,
            "{:.2} MB | {} pieces | {} chunks ({} fast, {} slow) | {:.1?} | {:.1} MB/s",
            mb,
            self.pieces,
            self.chunks_read,
            self.fast_chunks,
            self.slow_chunks,
            self.elapsed,
            throughput,
        )
    }
}

/// Rewrite the display stream, skipping tombstones.
pub fn save_rewrite(document: &mut Document, target: &Path) -> HxResult<SaveProfile> {
    if target == document.path() {
        let tmp = temp_path_for(target);
        let profile = write_pieces(document, &tmp)?;
        if let Ok(metadata) = fs::metadata(target) {
            fs::set_permissions(&tmp, metadata.permissions())?;
        }
        fs::rename(&tmp, target)?;
        return Ok(profile);
    }

    write_pieces(document, target)
}

/// Walk logical chunks and write them in bulk to a filesystem path.
fn write_pieces(document: &mut Document, target: &Path) -> HxResult<SaveProfile> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(target)?;
    let mut writer = BufWriter::new(file);
    write_pieces_to_writer(document, &mut writer)
}

/// Walk logical chunks and write them in bulk to any sink.
pub(crate) fn write_pieces_to_writer(
    document: &mut Document,
    writer: &mut dyn Write,
) -> HxResult<SaveProfile> {
    let save_start = Instant::now();

    const CHUNK: usize = 64 * 1024; // 64 KB read chunks

    let mut bytes_written: u64 = 0;
    let end = document.len().saturating_sub(1);
    let stats = document.walk_logical_chunks(0, end, CHUNK, |chunk| {
        if !chunk.bytes.is_empty() {
            writer.write_all(chunk.bytes)?;
            bytes_written += chunk.bytes.len() as u64;
        }
        Ok(WalkControl::Continue)
    })?;

    writer.flush()?;

    Ok(SaveProfile {
        bytes_written,
        pieces: stats.pieces,
        chunks_read: stats.chunks,
        fast_chunks: stats.fast_chunks,
        slow_chunks: stats.slow_chunks,
        elapsed: save_start.elapsed(),
    })
}

fn temp_path_for(target: &Path) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut name = target
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "hxedit.tmp".into());
    name.push(format!(".hxedit.tmp.{stamp}"));
    target.with_file_name(name)
}
