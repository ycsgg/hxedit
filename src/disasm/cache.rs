use std::collections::{BTreeMap, BTreeSet};

use crate::core::document::Document;
use crate::disasm::backend::DisassemblerBackend;
use crate::disasm::decode::decode_region_rows;
use crate::disasm::regions::{data_row_start, region_containing_or_after, visible_regions};
use crate::disasm::DisasmRow;
use crate::error::HxResult;
use crate::executable::ExecutableInfo;

const PREFETCH_ROWS: usize = 32;
const CHECKPOINT_STRIDE: usize = 32;
const DEFAULT_MAX_ROWS: usize = 8192;

#[derive(Debug)]
pub struct DisasmCache {
    rows: BTreeMap<u64, DisasmRow>,
    checkpoints: BTreeSet<u64>,
    region_starts: BTreeSet<u64>,
    max_rows: usize,
}

impl Default for DisasmCache {
    fn default() -> Self {
        Self {
            rows: BTreeMap::new(),
            checkpoints: BTreeSet::new(),
            region_starts: BTreeSet::new(),
            max_rows: DEFAULT_MAX_ROWS,
        }
    }
}

impl DisasmCache {
    pub fn new(info: &ExecutableInfo, doc_len: u64) -> Self {
        Self::with_max_rows_inner(info, doc_len, DEFAULT_MAX_ROWS)
    }

    #[cfg(all(test, feature = "disasm-capstone"))]
    fn with_max_rows(info: &ExecutableInfo, doc_len: u64, max_rows: usize) -> Self {
        Self::with_max_rows_inner(info, doc_len, max_rows)
    }

    fn with_max_rows_inner(info: &ExecutableInfo, doc_len: u64, max_rows: usize) -> Self {
        let mut cache = Self {
            max_rows: max_rows.max(PREFETCH_ROWS),
            ..Self::default()
        };
        cache.reset(info, doc_len);
        cache
    }

    pub fn reset(&mut self, info: &ExecutableInfo, doc_len: u64) {
        self.rows.clear();
        self.checkpoints.clear();
        self.region_starts.clear();
        for region in visible_regions(info, doc_len) {
            self.checkpoints.insert(region.start);
            self.region_starts.insert(region.start);
        }
    }

    #[cfg(all(test, feature = "disasm-capstone"))]
    pub(crate) fn cached_row_count(&self) -> usize {
        self.rows.len()
    }

    #[cfg(all(test, feature = "disasm-capstone"))]
    pub(crate) fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    pub fn collect_rows(
        &mut self,
        doc: &mut Document,
        info: &ExecutableInfo,
        backend: &dyn DisassemblerBackend,
        start: u64,
        row_count: usize,
    ) -> HxResult<Vec<DisasmRow>> {
        if row_count == 0 || doc.is_empty() {
            return Ok(Vec::new());
        }

        let start = start.min(doc.len().saturating_sub(1));
        let Some(mut cursor) = self.row_start_covering_or_after(doc, info, backend, start)? else {
            return Ok(Vec::new());
        };

        self.ensure_rows_cached(doc, info, backend, cursor, row_count)?;

        let mut rows = Vec::with_capacity(row_count);
        while rows.len() < row_count {
            let Some(row) = self.rows.get(&cursor).cloned() else {
                break;
            };
            let next = row.offset.saturating_add(row.len() as u64);
            rows.push(row);
            if next <= cursor {
                break;
            }
            cursor = next;
        }
        Ok(rows)
    }

    fn row_start_covering_or_after(
        &mut self,
        doc: &mut Document,
        info: &ExecutableInfo,
        backend: &dyn DisassemblerBackend,
        offset: u64,
    ) -> HxResult<Option<u64>> {
        if doc.is_empty() {
            return Ok(None);
        }

        let offset = offset.min(doc.len().saturating_sub(1));
        if let Some(row) = self.cached_row_containing(offset) {
            return Ok(Some(row.offset));
        }

        let Some(region) = region_containing_or_after(info, doc.len(), offset) else {
            return Ok(None);
        };
        if !region.executable {
            return Ok(Some(data_row_start(region.start, offset.max(region.start))));
        }

        let mut cursor = self
            .cached_prev_row_start(offset)
            .filter(|start| *start >= region.start)
            .or_else(|| {
                self.checkpoints
                    .range(region.start..=offset)
                    .next_back()
                    .copied()
            })
            .unwrap_or(region.start);

        loop {
            let row = if let Some(row) = self.rows.get(&cursor).cloned() {
                row
            } else {
                let batch_rows = PREFETCH_ROWS.max(8);
                let decoded = decode_region_rows(doc, info, backend, cursor, batch_rows)?;
                if decoded.is_empty() {
                    return Ok(None);
                }
                self.insert_rows(decoded, cursor, Some((cursor, cursor)));
                match self.rows.get(&cursor).cloned() {
                    Some(row) => row,
                    None => return Ok(None),
                }
            };

            let row_end = row
                .offset
                .saturating_add(row.len() as u64)
                .saturating_sub(1);
            if offset <= row_end {
                return Ok(Some(row.offset));
            }

            let next = row.offset.saturating_add(row.len() as u64);
            if next <= cursor || next > region.end_inclusive {
                return Ok(None);
            }
            cursor = next;
        }
    }

    fn ensure_rows_cached(
        &mut self,
        doc: &mut Document,
        info: &ExecutableInfo,
        backend: &dyn DisassemblerBackend,
        start: u64,
        row_count: usize,
    ) -> HxResult<()> {
        let target_rows = row_count.saturating_add(PREFETCH_ROWS).min(self.max_rows);
        let mut cursor = start;
        let mut cached = 0usize;

        while cached < target_rows && cursor < doc.len() {
            if let Some(row) = self.rows.get(&cursor) {
                cached += 1;
                let next = row.offset.saturating_add(row.len() as u64);
                if next <= cursor {
                    break;
                }
                cursor = next;
                continue;
            }

            let batch = (target_rows - cached).clamp(PREFETCH_ROWS, 256);
            let decoded = decode_region_rows(doc, info, backend, cursor, batch)?;
            if decoded.is_empty() {
                break;
            }
            self.insert_rows(decoded, cursor, Some((start, cursor)));
        }

        Ok(())
    }

    fn insert_rows(&mut self, rows: Vec<DisasmRow>, anchor: u64, protected: Option<(u64, u64)>) {
        for (idx, row) in rows.into_iter().enumerate() {
            if idx == 0 || idx % CHECKPOINT_STRIDE == 0 {
                self.checkpoints.insert(row.offset);
            }
            self.rows.insert(row.offset, row);
        }
        self.prune_around(anchor, protected);
    }

    fn prune_around(&mut self, anchor: u64, protected: Option<(u64, u64)>) {
        self.prune_rows_around(anchor, protected);
        self.prune_checkpoints_around(anchor);
    }

    fn prune_rows_around(&mut self, anchor: u64, protected: Option<(u64, u64)>) {
        while self.rows.len() > self.max_rows {
            let Some(first) = self
                .rows
                .keys()
                .find(|offset| !Self::is_protected(**offset, protected))
                .copied()
            else {
                break;
            };
            let Some(last) = self
                .rows
                .keys()
                .rev()
                .find(|offset| !Self::is_protected(**offset, protected))
                .copied()
            else {
                break;
            };
            let remove = if first.abs_diff(anchor) > last.abs_diff(anchor) {
                first
            } else {
                last
            };
            self.rows.remove(&remove);
        }
    }

    fn is_protected(offset: u64, protected: Option<(u64, u64)>) -> bool {
        protected.is_some_and(|(start, end)| offset >= start && offset <= end)
    }

    fn prune_checkpoints_around(&mut self, anchor: u64) {
        while self.non_region_checkpoint_count() > self.max_rows {
            let Some(first) = self
                .checkpoints
                .iter()
                .find(|offset| !self.region_starts.contains(*offset))
                .copied()
            else {
                break;
            };
            let Some(last) = self
                .checkpoints
                .iter()
                .rev()
                .find(|offset| !self.region_starts.contains(*offset))
                .copied()
            else {
                break;
            };
            let remove = if first.abs_diff(anchor) > last.abs_diff(anchor) {
                first
            } else {
                last
            };
            self.checkpoints.remove(&remove);
        }
    }

    fn non_region_checkpoint_count(&self) -> usize {
        self.checkpoints
            .iter()
            .filter(|offset| !self.region_starts.contains(*offset))
            .count()
    }

    fn cached_prev_row_start(&self, offset: u64) -> Option<u64> {
        self.rows
            .range(..=offset)
            .next_back()
            .map(|(start, _)| *start)
    }

    fn cached_row_containing(&self, offset: u64) -> Option<&DisasmRow> {
        let (_, row) = self.rows.range(..=offset).next_back()?;
        let row_end = row
            .offset
            .saturating_add(row.len() as u64)
            .saturating_sub(1);
        (offset <= row_end).then_some(row)
    }
}

#[cfg(all(test, feature = "disasm-capstone"))]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::DisasmCache;
    use crate::cli::Cli;
    use crate::core::document::Document;
    use crate::disasm::backend::resolve_backend;
    use crate::executable::detect_executable_info;

    fn doc_with_bytes(bytes: &[u8]) -> Document {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, bytes).unwrap();
        let cli = Cli {
            file: Some(file),
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
        };
        Document::open(cli.file.as_ref().unwrap(), &cli.config().unwrap()).unwrap()
    }

    fn x86_64_elf(code: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0_u8; (0x100 + code.len()).max(0x200)];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        let ph = 64usize;
        bytes[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[ph + 4..ph + 8].copy_from_slice(&0x5u32.to_le_bytes());
        bytes[ph + 8..ph + 16].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&(code.len() as u64).to_le_bytes());
        bytes[0x100..0x100 + code.len()].copy_from_slice(code);
        bytes
    }

    #[test]
    fn collect_rows_aligns_mid_instruction_offsets_to_containing_row() {
        let mut doc = doc_with_bytes(&x86_64_elf(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let mut cache = DisasmCache::new(&info, doc.len());

        let rows = cache
            .collect_rows(&mut doc, &info, backend.as_ref(), 0x102, 3)
            .unwrap();

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].offset, 0x101);
        assert_eq!(rows[1].offset, 0x104);
        assert_eq!(rows[2].offset, 0x105);
    }

    #[test]
    fn collect_rows_uses_cached_row_boundaries_for_repeated_queries() {
        let mut doc = doc_with_bytes(&x86_64_elf(&[0x55, 0x48, 0x89, 0xe5, 0x90, 0xc3]));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let mut cache = DisasmCache::new(&info, doc.len());

        let first = cache
            .collect_rows(&mut doc, &info, backend.as_ref(), 0x100, 4)
            .unwrap();
        let second = cache
            .collect_rows(&mut doc, &info, backend.as_ref(), 0x102, 2)
            .unwrap();

        assert_eq!(first[0].offset, 0x100);
        assert_eq!(first[1].offset, 0x101);
        assert_eq!(second[0].offset, 0x101);
        assert_eq!(second[1].offset, 0x104);
    }

    #[test]
    fn cache_prunes_rows_without_losing_decoding_coverage() {
        let code = vec![0x90; 4096];
        let mut doc = doc_with_bytes(&x86_64_elf(&code));
        let info = detect_executable_info(&mut doc).unwrap();
        let backend = resolve_backend(&info, None).unwrap();
        let mut cache = DisasmCache::with_max_rows(&info, doc.len(), 64);

        for start in (0x100_u64..0x900).step_by(64) {
            let rows = cache
                .collect_rows(&mut doc, &info, backend.as_ref(), start, 32)
                .unwrap();
            assert!(!rows.is_empty());
            assert!(cache.cached_row_count() <= 64);
            assert!(cache.checkpoint_count() <= cache.region_starts.len() + 64);
        }

        let rows = cache
            .collect_rows(&mut doc, &info, backend.as_ref(), 0x100, 8)
            .unwrap();
        assert_eq!(rows[0].offset, 0x100);
        assert!(cache.cached_row_count() <= 64);
        assert!(cache.checkpoint_count() <= cache.region_starts.len() + 64);
    }
}
