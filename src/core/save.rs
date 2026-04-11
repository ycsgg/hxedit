use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::document::Document;
use crate::error::HxResult;

/// Write only replacement patches back to the original file.
pub fn save_in_place(document: &Document, path: &Path) -> HxResult<()> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut current_start = None;
    let mut current_buf = Vec::new();
    let mut previous_offset = 0;

    for (&offset, &value) in document.patches().replacements() {
        match current_start {
            Some(_) if offset == previous_offset + 1 => {
                current_buf.push(value);
            }
            Some(start) => {
                file.seek(SeekFrom::Start(start))?;
                file.write_all(&current_buf)?;
                current_start = Some(offset);
                current_buf.clear();
                current_buf.push(value);
            }
            None => {
                current_start = Some(offset);
                current_buf.push(value);
            }
        }
        previous_offset = offset;
    }

    if let Some(start) = current_start {
        file.seek(SeekFrom::Start(start))?;
        file.write_all(&current_buf)?;
    }
    file.flush()?;
    Ok(())
}

/// Rewrite the logical document stream, dropping tombstoned bytes.
pub fn save_rewrite(document: &mut Document, target: &Path) -> HxResult<()> {
    if target == document.path() {
        let tmp = temp_path_for(target);
        write_virtual(document, &tmp)?;
        fs::rename(&tmp, target)?;
        return Ok(());
    }

    write_virtual(document, target)
}

fn write_virtual(document: &mut Document, target: &Path) -> HxResult<()> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(target)?;
    let mut writer = BufWriter::new(file);
    let chunk_size = 64 * 1024;
    let mut offset = 0_u64;

    while offset < document.original_len() {
        let bytes = document.raw_range(offset, chunk_size)?;
        if bytes.is_empty() {
            break;
        }
        for (idx, mut byte) in bytes.into_iter().enumerate() {
            let absolute = offset + idx as u64;
            if document.patches().is_deleted(absolute) {
                continue;
            }
            if let Some(replacement) = document.patches().replacement_at(absolute) {
                byte = replacement;
            }
            writer.write_all(&[byte])?;
        }
        offset += chunk_size as u64;
    }

    writer.flush()?;
    Ok(())
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
