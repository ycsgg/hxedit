use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::types::*;

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const DATABASE_HEADER_LEN: u64 = 100;
const BTREE_LEAF_HEADER_LEN: u64 = 8;
const BTREE_INTERIOR_HEADER_LEN: u64 = 12;

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < DATABASE_HEADER_LEN {
        return None;
    }

    let header = read_bytes_raw(doc, 0, DATABASE_HEADER_LEN as usize)?;
    if &header[0..16] != SQLITE_MAGIC {
        return None;
    }

    let page_size = parse_page_size(&header)?;
    let mut structs = vec![database_header_struct()];
    let file_pages = doc.len().div_ceil(page_size);
    let page_cap = entry_cap.max(1) as u64;
    let pages_to_parse = file_pages.min(page_cap);

    for page_index in 0..pages_to_parse {
        structs.push(page_struct(doc, page_index, page_size, entry_cap.max(1)));
    }

    if file_pages > pages_to_parse {
        structs.push(StructDef {
            name: format!(
                "... more SQLite pages beyond {} (use `:insp more` to load more)",
                pages_to_parse
            ),
            base_offset: pages_to_parse * page_size,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "SQLite".to_string(),
        structs,
    })
}

fn database_header_struct() -> StructDef {
    StructDef {
        name: "SQLite Database Header".into(),
        base_offset: 0,
        fields: vec![
            field(
                "signature",
                0,
                FieldType::Bytes(16),
                "SQLite database magic header string",
                false,
            ),
            field(
                "page_size",
                16,
                FieldType::U16Be,
                "Database page size; raw value 1 means 65536 bytes",
                true,
            ),
            field(
                "write_version",
                18,
                FieldType::custom_enum(
                    FieldType::U8,
                    vec![(1, "rollback journal".into()), (2, "WAL".into())],
                ),
                "File format write version",
                true,
            ),
            field(
                "read_version",
                19,
                FieldType::custom_enum(
                    FieldType::U8,
                    vec![(1, "rollback journal".into()), (2, "WAL".into())],
                ),
                "File format read version",
                true,
            ),
            field(
                "reserved_space",
                20,
                FieldType::U8,
                "Reserved bytes at the end of each page",
                true,
            ),
            field(
                "max_embedded_payload_fraction",
                21,
                FieldType::U8,
                "Maximum embedded payload fraction",
                true,
            ),
            field(
                "min_embedded_payload_fraction",
                22,
                FieldType::U8,
                "Minimum embedded payload fraction",
                true,
            ),
            field(
                "leaf_payload_fraction",
                23,
                FieldType::U8,
                "Leaf payload fraction",
                true,
            ),
            field(
                "file_change_counter",
                24,
                FieldType::U32Be,
                "File change counter",
                true,
            ),
            field(
                "database_size_pages",
                28,
                FieldType::U32Be,
                "Database size in pages according to the header",
                true,
            ),
            field(
                "first_freelist_trunk_page",
                32,
                FieldType::U32Be,
                "First freelist trunk page number",
                true,
            ),
            field(
                "total_freelist_pages",
                36,
                FieldType::U32Be,
                "Total number of freelist pages",
                true,
            ),
            field("schema_cookie", 40, FieldType::U32Be, "Schema cookie", true),
            field(
                "schema_format",
                44,
                FieldType::custom_enum(
                    FieldType::U32Be,
                    vec![
                        (1, "legacy".into()),
                        (2, "without rowid".into()),
                        (3, "descending indexes".into()),
                        (4, "modern".into()),
                    ],
                ),
                "Schema format number",
                true,
            ),
            field(
                "default_page_cache_size",
                48,
                FieldType::I32Be,
                "Suggested default page cache size",
                true,
            ),
            field(
                "largest_root_btree_page",
                52,
                FieldType::U32Be,
                "Largest root b-tree page when auto/incremental vacuum is active",
                true,
            ),
            field(
                "text_encoding",
                56,
                FieldType::custom_enum(
                    FieldType::U32Be,
                    vec![
                        (1, "UTF-8".into()),
                        (2, "UTF-16le".into()),
                        (3, "UTF-16be".into()),
                    ],
                ),
                "Database text encoding",
                true,
            ),
            field("user_version", 60, FieldType::U32Be, "User version", true),
            field(
                "incremental_vacuum_mode",
                64,
                FieldType::U32Be,
                "Incremental-vacuum mode flag",
                true,
            ),
            field(
                "application_id",
                68,
                FieldType::U32Be,
                "Application ID",
                true,
            ),
            field(
                "reserved_expansion",
                72,
                FieldType::Bytes(20),
                "Reserved expansion bytes; must normally be zero",
                false,
            ),
            field(
                "version_valid_for",
                92,
                FieldType::U32Be,
                "File change counter value that validates sqlite_version_number",
                true,
            ),
            field(
                "sqlite_version_number",
                96,
                FieldType::U32Be,
                "SQLite library version that last wrote the database",
                true,
            ),
        ],
        children: vec![],
    }
}

fn page_struct(
    doc: &mut Document,
    page_index: u64,
    page_size: u64,
    pointer_cap: usize,
) -> StructDef {
    let page_number = page_index + 1;
    let page_start = page_index * page_size;
    let page_end = page_start.saturating_add(page_size).min(doc.len());
    let header_offset = if page_index == 0 {
        DATABASE_HEADER_LEN
    } else {
        0
    };

    if page_end < page_start + header_offset + BTREE_LEAF_HEADER_LEN {
        return generic_page_struct(
            page_number,
            page_start,
            page_end.saturating_sub(page_start),
            "truncated",
        );
    }

    let Some(page_type) =
        read_bytes_raw(doc, page_start + header_offset, 1).and_then(|bytes| bytes.first().copied())
    else {
        return generic_page_struct(
            page_number,
            page_start,
            page_end.saturating_sub(page_start),
            "unreadable",
        );
    };

    let Some(kind) = BtreePageKind::from_byte(page_type) else {
        return generic_page_struct(
            page_number,
            page_start,
            page_end.saturating_sub(page_start),
            "non-b-tree or unknown",
        );
    };

    build_btree_page_struct(
        doc,
        page_number,
        page_start,
        page_end,
        header_offset,
        kind,
        pointer_cap,
    )
}

fn build_btree_page_struct(
    doc: &mut Document,
    page_number: u64,
    page_start: u64,
    page_end: u64,
    header_offset: u64,
    kind: BtreePageKind,
    pointer_cap: usize,
) -> StructDef {
    let header_len = kind.header_len();
    let truncated_header = page_end < page_start + header_offset + header_len;
    let header_bytes = read_bytes_raw(
        doc,
        page_start + header_offset,
        if truncated_header {
            BTREE_LEAF_HEADER_LEN as usize
        } else {
            header_len as usize
        },
    )
    .unwrap_or_default();

    let cell_count = header_bytes
        .get(3..5)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        .unwrap_or(0);
    let cell_content_raw = header_bytes
        .get(5..7)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        .unwrap_or(0);
    let cell_content_area = if cell_content_raw == 0 && page_end - page_start == 65_536 {
        65_536
    } else {
        cell_content_raw as u64
    };
    let pointer_array_offset = header_offset + header_len;
    let pointer_array_len = cell_count as u64 * 2;
    let available_pointer_bytes = page_end
        .saturating_sub(page_start)
        .saturating_sub(pointer_array_offset)
        .min(pointer_array_len);
    let available_pointers = available_pointer_bytes / 2;
    let pointers_to_show = available_pointers.min(pointer_cap as u64);

    let mut fields = vec![
        field(
            "page_type",
            header_offset,
            FieldType::custom_enum(FieldType::U8, btree_page_type_variants()),
            "SQLite b-tree page type",
            true,
        ),
        field(
            "first_freeblock",
            header_offset + 1,
            FieldType::U16Be,
            "Offset of the first freeblock within this page, or zero",
            true,
        ),
        field(
            "cell_count",
            header_offset + 3,
            FieldType::U16Be,
            "Number of cells on this b-tree page",
            true,
        ),
        field(
            "cell_content_area",
            header_offset + 5,
            FieldType::U16Be,
            "Start offset of the cell content area within this page",
            true,
        ),
        field(
            "fragmented_free_bytes",
            header_offset + 7,
            FieldType::U8,
            "Number of fragmented free bytes on this page",
            true,
        ),
    ];

    if kind.is_interior() && !truncated_header {
        fields.push(field(
            "rightmost_pointer",
            header_offset + 8,
            FieldType::U32Be,
            "Right-most child page number for an interior b-tree page",
            true,
        ));
    }

    if pointer_array_len > 0 && available_pointer_bytes > 0 {
        fields.push(field(
            "cell_pointer_array",
            pointer_array_offset,
            FieldType::DataRange(available_pointer_bytes),
            "Cell pointer array; entries are 2-byte offsets within this page",
            false,
        ));
        for index in 0..pointers_to_show {
            fields.push(field(
                &format!("cell_{index}_offset"),
                pointer_array_offset + index * 2,
                FieldType::U16Be,
                "Cell content offset within this page; payload is not decoded",
                false,
            ));
        }
        if available_pointers > pointers_to_show {
            fields.push(field(
                "remaining_cell_pointers",
                pointer_array_offset + pointers_to_show * 2,
                FieldType::DataRange((available_pointers - pointers_to_show) * 2),
                "Additional cell pointer entries hidden by the inspector entry cap",
                false,
            ));
        }
    }

    let page_bytes = page_end.saturating_sub(page_start);
    if cell_content_area < page_bytes && cell_content_area >= pointer_array_offset {
        fields.push(field(
            "cell_content_region",
            cell_content_area,
            FieldType::DataRange(page_bytes - cell_content_area),
            "Raw b-tree cell content region; record payload is intentionally not decoded",
            false,
        ));
    }

    let mut name = format!("Page {page_number}: {}", kind.label());
    if truncated_header || available_pointer_bytes < pointer_array_len {
        name.push_str(" (truncated)");
    }

    StructDef {
        name,
        base_offset: page_start,
        fields,
        children: vec![],
    }
}

fn generic_page_struct(
    page_number: u64,
    page_start: u64,
    available_len: u64,
    label: &str,
) -> StructDef {
    let fields = if available_len > 0 {
        vec![field(
            "page_bytes",
            0,
            FieldType::DataRange(available_len),
            "Raw page bytes; this page is not decoded as a b-tree page",
            false,
        )]
    } else {
        Vec::new()
    };

    StructDef {
        name: format!("Page {page_number}: {label}"),
        base_offset: page_start,
        fields,
        children: vec![],
    }
}

#[derive(Clone, Copy)]
enum BtreePageKind {
    InteriorIndex,
    InteriorTable,
    LeafIndex,
    LeafTable,
}

impl BtreePageKind {
    const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x02 => Some(Self::InteriorIndex),
            0x05 => Some(Self::InteriorTable),
            0x0a => Some(Self::LeafIndex),
            0x0d => Some(Self::LeafTable),
            _ => None,
        }
    }

    const fn is_interior(self) -> bool {
        matches!(self, Self::InteriorIndex | Self::InteriorTable)
    }

    const fn header_len(self) -> u64 {
        if self.is_interior() {
            BTREE_INTERIOR_HEADER_LEN
        } else {
            BTREE_LEAF_HEADER_LEN
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::InteriorIndex => "b-tree interior index",
            Self::InteriorTable => "b-tree interior table",
            Self::LeafIndex => "b-tree leaf index",
            Self::LeafTable => "b-tree leaf table",
        }
    }
}

fn btree_page_type_variants() -> Vec<(u64, String)> {
    vec![
        (0x02, "Interior index b-tree".into()),
        (0x05, "Interior table b-tree".into()),
        (0x0a, "Leaf index b-tree".into()),
        (0x0d, "Leaf table b-tree".into()),
    ]
}

fn parse_page_size(header: &[u8]) -> Option<u64> {
    let raw = u16::from_be_bytes([header[16], header[17]]);
    match raw {
        1 => Some(65_536),
        512..=32_768 if raw.is_power_of_two() => Some(raw as u64),
        _ => None,
    }
}

fn field(
    name: &str,
    offset: u64,
    field_type: FieldType,
    description: &str,
    editable: bool,
) -> FieldDef {
    FieldDef {
        name: name.into(),
        offset,
        field_type,
        description: description.into(),
        editable,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format::types::FieldType;

    fn open_sqlite(bytes: Vec<u8>) -> Document {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.sqlite");
        fs::write(&path, bytes).unwrap();
        Document::open(&path, &Config::default()).unwrap()
    }

    fn sqlite_header(page_size: u16, page_count: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; 100];
        bytes[0..16].copy_from_slice(super::SQLITE_MAGIC);
        bytes[16..18].copy_from_slice(&page_size.to_be_bytes());
        bytes[18] = 1;
        bytes[19] = 1;
        bytes[20] = 0;
        bytes[21] = 64;
        bytes[22] = 32;
        bytes[23] = 32;
        bytes[28..32].copy_from_slice(&page_count.to_be_bytes());
        bytes[44..48].copy_from_slice(&4_u32.to_be_bytes());
        bytes[56..60].copy_from_slice(&1_u32.to_be_bytes());
        bytes[96..100].copy_from_slice(&3_046_000_u32.to_be_bytes());
        bytes
    }

    fn write_leaf_page(page: &mut [u8], header_offset: usize, cell_pointers: &[u16]) {
        page[header_offset] = 0x0d;
        page[header_offset + 1..header_offset + 3].copy_from_slice(&0_u16.to_be_bytes());
        page[header_offset + 3..header_offset + 5]
            .copy_from_slice(&(cell_pointers.len() as u16).to_be_bytes());
        page[header_offset + 5..header_offset + 7].copy_from_slice(&400_u16.to_be_bytes());
        page[header_offset + 7] = 0;
        for (index, pointer) in cell_pointers.iter().enumerate() {
            let offset = header_offset + 8 + index * 2;
            page[offset..offset + 2].copy_from_slice(&pointer.to_be_bytes());
        }
    }

    #[test]
    fn parses_database_header_and_leaf_btree_page_without_record_payload() {
        let mut bytes = sqlite_header(512, 1);
        bytes.resize(512, 0);
        write_leaf_page(&mut bytes, 100, &[400, 420]);

        let mut doc = open_sqlite(bytes);
        let def = detect(&mut doc).expect("sqlite should be detected");

        assert_eq!(def.name, "SQLite");
        assert_eq!(def.structs[0].name, "SQLite Database Header");
        let page = def
            .structs
            .iter()
            .find(|structure| structure.name == "Page 1: b-tree leaf table")
            .expect("page 1 b-tree");
        assert!(page
            .fields
            .iter()
            .any(|field| field.name == "cell_0_offset"
                && matches!(field.field_type, FieldType::U16Be)));
        assert!(!page
            .fields
            .iter()
            .any(|field| field.name.contains("record")));
    }

    #[test]
    fn paginates_pages_and_parses_interior_rightmost_pointer() {
        let mut bytes = sqlite_header(512, 2);
        bytes.resize(1024, 0);
        write_leaf_page(&mut bytes[0..512], 100, &[400]);
        let page2 = &mut bytes[512..1024];
        page2[0] = 0x05;
        page2[1..3].copy_from_slice(&0_u16.to_be_bytes());
        page2[3..5].copy_from_slice(&1_u16.to_be_bytes());
        page2[5..7].copy_from_slice(&480_u16.to_be_bytes());
        page2[7] = 0;
        page2[8..12].copy_from_slice(&3_u32.to_be_bytes());
        page2[12..14].copy_from_slice(&480_u16.to_be_bytes());

        let mut doc = open_sqlite(bytes);
        let def = detect_with_cap(&mut doc, 1).expect("sqlite should be detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("more SQLite pages beyond 1")));

        let mut doc = open_sqlite({
            let mut full = sqlite_header(512, 2);
            full.resize(1024, 0);
            write_leaf_page(&mut full[0..512], 100, &[400]);
            let page2 = &mut full[512..1024];
            page2[0] = 0x05;
            page2[3..5].copy_from_slice(&1_u16.to_be_bytes());
            page2[5..7].copy_from_slice(&480_u16.to_be_bytes());
            page2[8..12].copy_from_slice(&3_u32.to_be_bytes());
            page2[12..14].copy_from_slice(&480_u16.to_be_bytes());
            full
        });
        let def = detect(&mut doc).expect("sqlite should be detected");
        let page = def
            .structs
            .iter()
            .find(|structure| structure.name == "Page 2: b-tree interior table")
            .expect("page 2 interior b-tree");
        assert!(page
            .fields
            .iter()
            .any(|field| field.name == "rightmost_pointer"));
    }

    #[test]
    fn rejects_invalid_page_size() {
        let mut bytes = sqlite_header(500, 1);
        bytes.resize(512, 0);
        let mut doc = open_sqlite(bytes);
        assert!(detect(&mut doc).is_none());
    }
}
