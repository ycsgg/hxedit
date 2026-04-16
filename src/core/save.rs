use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Walk pieces directly and write in bulk chunks.
///
/// For each piece we read/slice a whole contiguous run at once.  Before
/// touching individual bytes we do a cheap O(log n) range query on the
/// tombstone and replacement sets.  If neither set intersects the current
/// chunk we write the raw bytes directly — this is the common case and
/// makes saving a 16 MB file with a single tombstone almost as fast as
/// saving a completely clean file.
fn write_pieces(document: &mut Document, target: &Path) -> HxResult<SaveProfile> {
    use crate::core::piece_table::{CellId, PieceSource};

    let save_start = Instant::now();

    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(target)?;
    let mut writer = BufWriter::new(file);

    let pieces: Vec<_> = document.pieces_snapshot();
    let has_tombstones = document.has_tombstones();
    let has_replacements = document.has_replacements();

    const CHUNK: usize = 64 * 1024; // 64 KB read chunks

    let mut bytes_written: u64 = 0;
    let mut pieces_count: usize = 0;
    let mut chunks_read: usize = 0;
    let mut fast_chunks: usize = 0;
    let mut slow_chunks: usize = 0;

    for piece in &pieces {
        pieces_count += 1;
        match piece.source {
            PieceSource::Original => {
                let mut remaining = piece.len;
                let mut file_off = piece.start;
                let mut cell_off = file_off;

                while remaining > 0 {
                    let batch = (remaining as usize).min(CHUNK);
                    let raw = document.raw_range(file_off, batch)?;
                    if raw.is_empty() {
                        break;
                    }
                    chunks_read += 1;
                    let read_len = raw.len() as u64;

                    // Range check: does this chunk intersect any tombstone or replacement?
                    let chunk_lo = CellId::Original(cell_off);
                    let chunk_hi = CellId::Original(cell_off + read_len - 1);
                    let need_tombstone_scan =
                        has_tombstones && document.has_tombstone_in_range(chunk_lo, chunk_hi);
                    let need_replacement_scan =
                        has_replacements && document.has_replacement_in_range(chunk_lo, chunk_hi);

                    if !need_tombstone_scan && !need_replacement_scan {
                        // Fast path: write raw chunk directly.
                        writer.write_all(&raw)?;
                        bytes_written += read_len;
                        fast_chunks += 1;
                    } else {
                        // Slow path: buffer the survivors, then one write_all.
                        slow_chunks += 1;
                        let mut buf = Vec::with_capacity(raw.len());
                        for (i, &base) in raw.iter().enumerate() {
                            let id = CellId::Original(cell_off + i as u64);
                            if need_tombstone_scan && document.is_tombstone(id) {
                                continue;
                            }
                            let byte = if need_replacement_scan {
                                document.replacement_for(id).unwrap_or(base)
                            } else {
                                base
                            };
                            buf.push(byte);
                        }
                        writer.write_all(&buf)?;
                        bytes_written += buf.len() as u64;
                    }

                    file_off += read_len;
                    cell_off += read_len;
                    remaining -= read_len;
                }
            }
            PieceSource::Add => {
                let data = document.add_slice(piece.start, piece.len);
                let chunk_lo = CellId::Add(piece.start);
                let chunk_hi = CellId::Add(piece.start + piece.len.saturating_sub(1));
                let need_tombstone_scan =
                    has_tombstones && document.has_tombstone_in_range(chunk_lo, chunk_hi);
                let need_replacement_scan =
                    has_replacements && document.has_replacement_in_range(chunk_lo, chunk_hi);

                if !need_tombstone_scan && !need_replacement_scan {
                    writer.write_all(data)?;
                    bytes_written += data.len() as u64;
                } else {
                    let mut buf = Vec::with_capacity(data.len());
                    for (i, &base) in data.iter().enumerate() {
                        let id = CellId::Add(piece.start + i as u64);
                        if need_tombstone_scan && document.is_tombstone(id) {
                            continue;
                        }
                        let byte = if need_replacement_scan {
                            document.replacement_for(id).unwrap_or(base)
                        } else {
                            base
                        };
                        buf.push(byte);
                    }
                    writer.write_all(&buf)?;
                    bytes_written += buf.len() as u64;
                }
            }
        }
    }

    writer.flush()?;

    Ok(SaveProfile {
        bytes_written,
        pieces: pieces_count,
        chunks_read,
        fast_chunks,
        slow_chunks,
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
