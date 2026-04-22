use crate::executable::{CodeSpan, ExecutableInfo};

pub const DATA_ROW_BYTES: u64 = 8;

pub fn visible_regions(info: &ExecutableInfo, doc_len: u64) -> Vec<CodeSpan> {
    if doc_len == 0 {
        return Vec::new();
    }

    let mut spans = info.code_spans.clone();
    spans.sort_by_key(|span| (span.start, span.end_inclusive - span.start));

    let mut regions = Vec::new();
    let mut cursor = 0_u64;
    let file_end = doc_len.saturating_sub(1);

    for span in spans {
        if span.start > file_end {
            continue;
        }
        if span.start > cursor {
            regions.push(CodeSpan {
                start: cursor,
                end_inclusive: span.start - 1,
                virtual_start: None,
                virtual_end_inclusive: None,
                name: Some("<raw>".to_owned()),
                executable: false,
            });
        }

        let start = span.start.max(cursor);
        let end = span.end_inclusive.min(file_end);
        if start <= end {
            let virtual_start = span.virtual_address_for_offset(start);
            let virtual_end_inclusive = span.virtual_address_for_offset(end);
            regions.push(CodeSpan {
                start,
                end_inclusive: end,
                virtual_start,
                virtual_end_inclusive,
                name: span.name.clone(),
                executable: span.executable,
            });
            cursor = end.saturating_add(1);
        }

        if cursor > file_end {
            break;
        }
    }

    if cursor <= file_end {
        regions.push(CodeSpan {
            start: cursor,
            end_inclusive: file_end,
            virtual_start: None,
            virtual_end_inclusive: None,
            name: Some("<raw>".to_owned()),
            executable: false,
        });
    }

    regions
}

pub fn region_containing_or_after(
    info: &ExecutableInfo,
    doc_len: u64,
    offset: u64,
) -> Option<CodeSpan> {
    visible_regions(info, doc_len)
        .into_iter()
        .find(|region| region.contains(offset) || region.start >= offset)
}

pub fn data_row_start(region_start: u64, offset: u64) -> u64 {
    region_start + ((offset.saturating_sub(region_start)) / DATA_ROW_BYTES) * DATA_ROW_BYTES
}

pub fn data_row_offsets_before(
    region_start: u64,
    region_end: u64,
    limit_exclusive: u64,
) -> Vec<u64> {
    let capped_limit = limit_exclusive.min(region_end.saturating_add(1));
    if capped_limit <= region_start {
        return Vec::new();
    }
    let mut offsets = Vec::new();
    let mut current = region_start;
    while current < capped_limit {
        offsets.push(current);
        current = current.saturating_add(DATA_ROW_BYTES);
    }
    offsets
}

#[cfg(test)]
mod tests {
    use crate::executable::{
        Bitness, CodeSpan, Endian, ExecutableArch, ExecutableInfo, ExecutableKind,
    };

    use super::{data_row_offsets_before, data_row_start, visible_regions};

    fn info_with_spans(spans: Vec<CodeSpan>) -> ExecutableInfo {
        ExecutableInfo {
            kind: ExecutableKind::Elf,
            arch: ExecutableArch::X86_64,
            bitness: Bitness::Bit64,
            endian: Endian::Little,
            entry_offset: None,
            entry_virtual_address: None,
            code_spans: spans,
            symbols_by_va: Default::default(),
            target_names_by_va: Default::default(),
            symbols_by_name: Default::default(),
            imports: Vec::new(),
        }
    }

    #[test]
    fn visible_regions_fill_raw_gaps_and_tail() {
        let info = info_with_spans(vec![CodeSpan {
            start: 0x10,
            end_inclusive: 0x17,
            virtual_start: Some(0x4010),
            virtual_end_inclusive: Some(0x4017),
            name: Some(".text".to_owned()),
            executable: true,
        }]);

        let regions = visible_regions(&info, 0x20);
        assert_eq!(regions.len(), 3);
        assert_eq!((regions[0].start, regions[0].end_inclusive), (0, 0x0f));
        assert!(!regions[0].executable);
        assert_eq!(regions[1].name.as_deref(), Some(".text"));
        assert_eq!((regions[2].start, regions[2].end_inclusive), (0x18, 0x1f));
        assert!(!regions[2].executable);
    }

    #[test]
    fn raw_data_rows_keep_region_local_alignment() {
        assert_eq!(data_row_start(0x20, 0x20), 0x20);
        assert_eq!(data_row_start(0x20, 0x27), 0x20);
        assert_eq!(data_row_start(0x23, 0x2a), 0x2b - 8);
        assert_eq!(
            data_row_offsets_before(0x23, 0x33, 0x34),
            vec![0x23, 0x2b, 0x33]
        );
    }
}
