use crate::core::document::Document;
use crate::format::detect::read_bytes_raw;
use crate::format::time;
use crate::format::types::*;

const ZIP_LOCAL_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const ZIP_CENTRAL_MAGIC: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const ZIP_DATA_DESCRIPTOR_MAGIC: [u8; 4] = [0x50, 0x4b, 0x07, 0x08];
const ZIP64_EOCD_MAGIC: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
const ZIP64_LOCATOR_MAGIC: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
const ZIP_EOCD_MAGIC: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

const ZIP_DATA_DESCRIPTOR_FLAG: u16 = 0x0008;
const ZIP64_EXTRA_ID: u16 = 0x0001;

const LOCAL_FIXED_LEN: u64 = 30;
const CENTRAL_FIXED_LEN: u64 = 46;
const EOCD_FIXED_LEN: u64 = 22;
const ZIP64_LOCATOR_LEN: u64 = 20;
const ZIP64_EOCD_MIN_LEN: u64 = 56;
const ZIP_COMMENT_MAX: u64 = u16::MAX as u64;

/// Detect and parse ZIP format with the default entry cap.
pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, super::super::detect::DEFAULT_ENTRY_CAP)
}

/// Detect and parse ZIP format, stopping after `entry_cap` archive entries.
///
/// Complete archives are parsed from EOCD -> central directory, which lets the
/// inspector show data-descriptor and ZIP64 entries without guessing payload
/// length from a forward local-header scan. Truncated/streaming inputs without
/// EOCD still fall back to the conservative local-header scan.
pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < 4 {
        return None;
    }

    let first = read_bytes_raw(doc, 0, 4)?;
    if first != ZIP_LOCAL_MAGIC && first != ZIP_EOCD_MAGIC {
        return None;
    }

    if let Some(eocd) = find_eocd(doc) {
        if let Some(def) = detect_from_central_directory(doc, entry_cap, eocd.clone()) {
            return Some(def);
        }
    }

    if first == ZIP_LOCAL_MAGIC {
        return detect_local_headers_fallback(doc, entry_cap);
    }

    None
}

fn detect_from_central_directory(
    doc: &mut Document,
    entry_cap: usize,
    eocd: Eocd,
) -> Option<FormatDef> {
    let zip64 = parse_zip64_eocd(doc, &eocd);
    let central_offset = zip64
        .as_ref()
        .map_or(eocd.central_directory_offset as u64, |record| {
            record.central_directory_offset
        });
    let total_entries = zip64
        .as_ref()
        .map_or(eocd.total_entries as u64, |record| record.total_entries);

    let mut entries = Vec::new();
    let mut offset = central_offset;
    let max_entries = entry_cap.max(1) as u64;
    let entries_to_parse = total_entries.min(max_entries);
    for index in 0..entries_to_parse {
        let entry = parse_central_entry(doc, offset, index as usize)?;
        offset = entry.next_offset;
        entries.push(entry);
    }

    let more_remain = total_entries > entries.len() as u64
        || read_bytes_raw(doc, offset, 4).is_some_and(|sig| sig == ZIP_CENTRAL_MAGIC);

    let mut structs = Vec::new();
    for entry in &entries {
        if let Some(local) = build_local_file_structs(doc, entry) {
            structs.extend(local);
        }
    }
    for entry in &entries {
        structs.push(build_central_directory_struct(entry));
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "… more ZIP entries beyond {} (use `:insp more` to load more)",
                entries.len()
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    if let Some(record) = zip64 {
        structs.push(build_zip64_eocd_struct(&record));
        structs.push(build_zip64_locator_struct(&record));
    }
    structs.push(build_eocd_struct(&eocd));

    if structs.is_empty() {
        return None;
    }

    Some(FormatDef {
        name: "ZIP".to_string(),
        structs,
    })
}

fn detect_local_headers_fallback(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    let mut structs = Vec::new();
    let mut offset: u64 = 0;
    let mut entry_idx = 0;
    let mut more_remain = false;

    while offset + LOCAL_FIXED_LEN <= doc.len() {
        if entry_idx >= entry_cap.max(1) {
            if read_bytes_raw(doc, offset, 4).is_some_and(|sig| sig == ZIP_LOCAL_MAGIC) {
                more_remain = true;
            }
            break;
        }

        let header = parse_local_header(doc, offset)?;
        if header.signature != ZIP_LOCAL_MAGIC {
            break;
        }

        let compressed_size = header.compressed_size_32 as u64;
        let has_data_descriptor = header.flags & ZIP_DATA_DESCRIPTOR_FLAG != 0;
        let mut fields = local_header_fields(&header);
        if !has_data_descriptor && compressed_size > 0 {
            fields.push(FieldDef {
                name: "file_data".into(),
                offset: header.data_offset - offset,
                field_type: FieldType::DataRange(compressed_size),
                description: "Compressed file data".into(),
                editable: false,
            });
        }

        structs.push(StructDef {
            name: if has_data_descriptor {
                format!(
                    "Local File: {} [data descriptor; partial scan]",
                    header.display_name(entry_idx)
                )
            } else {
                format!("Local File: {}", header.display_name(entry_idx))
            },
            base_offset: offset,
            fields,
            children: vec![],
        });

        if has_data_descriptor {
            break;
        }

        offset = header.data_offset.checked_add(compressed_size)?;
        entry_idx += 1;
    }

    if structs.is_empty() {
        return None;
    }

    if more_remain {
        structs.push(StructDef {
            name: format!(
                "… more entries beyond {} (use `:insp more` to load more)",
                entry_idx
            ),
            base_offset: offset,
            fields: vec![],
            children: vec![],
        });
    }

    Some(FormatDef {
        name: "ZIP".to_string(),
        structs,
    })
}

#[derive(Clone)]
struct Eocd {
    offset: u64,
    total_entries: u16,
    central_directory_offset: u32,
    comment_len: u16,
}

struct Zip64Eocd {
    offset: u64,
    size_of_record: u64,
    total_entries: u64,
    central_directory_offset: u64,
    locator_offset: u64,
}

struct CentralEntry {
    index: usize,
    offset: u64,
    next_offset: u64,
    mod_time: u16,
    mod_date: u16,
    compressed_size_32: u32,
    uncompressed_size_32: u32,
    filename_len: u16,
    extra_len: u16,
    comment_len: u16,
    filename: Option<String>,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    zip64_layout: Zip64ExtraLayout,
}

#[derive(Default)]
struct Zip64ExtraLayout {
    uncompressed_size_offset: Option<usize>,
    compressed_size_offset: Option<usize>,
    local_header_offset_offset: Option<usize>,
    disk_start_offset: Option<usize>,
}

struct LocalHeader {
    signature: [u8; 4],
    offset: u64,
    flags: u16,
    mod_time: u16,
    mod_date: u16,
    compressed_size_32: u32,
    filename_len: u16,
    extra_len: u16,
    filename: Option<String>,
    data_offset: u64,
    zip64_layout: Zip64ExtraLayout,
}

fn find_eocd(doc: &mut Document) -> Option<Eocd> {
    if doc.len() < EOCD_FIXED_LEN {
        return None;
    }

    let search_len = doc.len().min(EOCD_FIXED_LEN + ZIP_COMMENT_MAX);
    let search_start = doc.len() - search_len;
    let bytes = read_bytes_raw(doc, search_start, search_len as usize)?;
    let last_sig_start = bytes.len().checked_sub(ZIP_EOCD_MAGIC.len())?;
    for index in (0..=last_sig_start).rev() {
        if bytes[index..index + 4] != ZIP_EOCD_MAGIC {
            continue;
        }
        if index + EOCD_FIXED_LEN as usize > bytes.len() {
            continue;
        }

        let comment_len = le_u16(&bytes, index + 20);
        let eocd_len = EOCD_FIXED_LEN + comment_len as u64;
        let offset = search_start + index as u64;
        if offset.checked_add(eocd_len)? != doc.len() {
            continue;
        }

        return Some(Eocd {
            offset,
            total_entries: le_u16(&bytes, index + 10),
            central_directory_offset: le_u32(&bytes, index + 16),
            comment_len,
        });
    }

    None
}

fn parse_zip64_eocd(doc: &mut Document, eocd: &Eocd) -> Option<Zip64Eocd> {
    let locator_offset = eocd.offset.checked_sub(ZIP64_LOCATOR_LEN)?;
    let locator = read_bytes_raw(doc, locator_offset, ZIP64_LOCATOR_LEN as usize)?;
    if locator[0..4] != ZIP64_LOCATOR_MAGIC {
        return None;
    }

    let zip64_offset = le_u64(&locator, 8);
    let fixed = read_bytes_raw(doc, zip64_offset, ZIP64_EOCD_MIN_LEN as usize)?;
    if fixed[0..4] != ZIP64_EOCD_MAGIC {
        return None;
    }

    Some(Zip64Eocd {
        offset: zip64_offset,
        size_of_record: le_u64(&fixed, 4),
        total_entries: le_u64(&fixed, 32),
        central_directory_offset: le_u64(&fixed, 48),
        locator_offset,
    })
}

fn parse_central_entry(doc: &mut Document, offset: u64, index: usize) -> Option<CentralEntry> {
    let fixed = read_bytes_raw(doc, offset, CENTRAL_FIXED_LEN as usize)?;
    if fixed[0..4] != ZIP_CENTRAL_MAGIC {
        return None;
    }

    let mod_time = le_u16(&fixed, 12);
    let mod_date = le_u16(&fixed, 14);
    let compressed_size_32 = le_u32(&fixed, 20);
    let uncompressed_size_32 = le_u32(&fixed, 24);
    let filename_len = le_u16(&fixed, 28);
    let extra_len = le_u16(&fixed, 30);
    let comment_len = le_u16(&fixed, 32);
    let disk_start_16 = le_u16(&fixed, 34);
    let local_header_offset_32 = le_u32(&fixed, 42);
    let variable_len = filename_len as u64 + extra_len as u64 + comment_len as u64;
    let next_offset = offset.checked_add(CENTRAL_FIXED_LEN + variable_len)?;
    if next_offset > doc.len() {
        return None;
    }

    let filename = read_filename(doc, offset + CENTRAL_FIXED_LEN, filename_len);
    let extra_offset = offset + CENTRAL_FIXED_LEN + filename_len as u64;
    let extra = read_bytes_raw(doc, extra_offset, extra_len as usize)?;
    let need_uncompressed = uncompressed_size_32 == u32::MAX;
    let need_compressed = compressed_size_32 == u32::MAX;
    let need_local = local_header_offset_32 == u32::MAX;
    let need_disk = disk_start_16 == u16::MAX;
    let zip64_layout = parse_zip64_extra_layout(
        &extra,
        need_uncompressed,
        need_compressed,
        need_local,
        need_disk,
    );

    let uncompressed_size = zip64_layout
        .uncompressed_size_offset
        .and_then(|field_offset| read_u64_from(&extra, field_offset))
        .unwrap_or(uncompressed_size_32 as u64);
    let compressed_size = zip64_layout
        .compressed_size_offset
        .and_then(|field_offset| read_u64_from(&extra, field_offset))
        .unwrap_or(compressed_size_32 as u64);
    let local_header_offset = zip64_layout
        .local_header_offset_offset
        .and_then(|field_offset| read_u64_from(&extra, field_offset))
        .unwrap_or(local_header_offset_32 as u64);

    Some(CentralEntry {
        index,
        offset,
        next_offset,
        mod_time,
        mod_date,
        compressed_size_32,
        uncompressed_size_32,
        filename_len,
        extra_len,
        comment_len,
        filename,
        compressed_size,
        uncompressed_size,
        local_header_offset,
        zip64_layout,
    })
}

fn parse_local_header(doc: &mut Document, offset: u64) -> Option<LocalHeader> {
    let fixed = read_bytes_raw(doc, offset, LOCAL_FIXED_LEN as usize)?;
    let signature = [fixed[0], fixed[1], fixed[2], fixed[3]];
    let flags = le_u16(&fixed, 6);
    let mod_time = le_u16(&fixed, 10);
    let mod_date = le_u16(&fixed, 12);
    let compressed_size_32 = le_u32(&fixed, 18);
    let uncompressed_size_32 = le_u32(&fixed, 22);
    let filename_len = le_u16(&fixed, 26);
    let extra_len = le_u16(&fixed, 28);
    let data_offset =
        offset.checked_add(LOCAL_FIXED_LEN + filename_len as u64 + extra_len as u64)?;
    if data_offset > doc.len() {
        return None;
    }

    let filename = read_filename(doc, offset + LOCAL_FIXED_LEN, filename_len);
    let extra_offset = offset + LOCAL_FIXED_LEN + filename_len as u64;
    let extra = read_bytes_raw(doc, extra_offset, extra_len as usize)?;
    let zip64_layout = parse_zip64_extra_layout(
        &extra,
        uncompressed_size_32 == u32::MAX,
        compressed_size_32 == u32::MAX,
        false,
        false,
    );

    Some(LocalHeader {
        signature,
        offset,
        flags,
        mod_time,
        mod_date,
        compressed_size_32,
        filename_len,
        extra_len,
        filename,
        data_offset,
        zip64_layout,
    })
}

fn build_local_file_structs(doc: &mut Document, entry: &CentralEntry) -> Option<Vec<StructDef>> {
    let header = parse_local_header(doc, entry.local_header_offset)?;
    if header.signature != ZIP_LOCAL_MAGIC {
        return None;
    }

    let mut structs = Vec::new();
    let mut fields = local_header_fields(&header);
    add_zip64_extra_fields(
        &mut fields,
        LOCAL_FIXED_LEN + header.filename_len as u64,
        &header.zip64_layout,
    );

    let data_end = header.data_offset.checked_add(entry.compressed_size)?;
    let truncated = data_end > doc.len();
    if entry.compressed_size > 0 && !truncated {
        fields.push(FieldDef {
            name: "file_data".into(),
            offset: header.data_offset - header.offset,
            field_type: FieldType::DataRange(entry.compressed_size),
            description: "Compressed file data, sized from the central directory".into(),
            editable: false,
        });
    }

    let has_data_descriptor = header.flags & ZIP_DATA_DESCRIPTOR_FLAG != 0;
    structs.push(StructDef {
        name: local_struct_name(entry, &header, truncated),
        base_offset: header.offset,
        fields,
        children: vec![],
    });

    if has_data_descriptor && !truncated {
        if let Some(descriptor) = build_data_descriptor_struct(doc, entry, data_end) {
            structs.push(descriptor);
        }
    }

    Some(structs)
}

fn build_central_directory_struct(entry: &CentralEntry) -> StructDef {
    let mut fields = vec![
        field(
            "signature",
            0,
            FieldType::Bytes(4),
            "Central directory file header signature",
            false,
        ),
        field(
            "version_made_by",
            4,
            FieldType::U16Le,
            "Version made by",
            false,
        ),
        field(
            "version_needed",
            6,
            FieldType::U16Le,
            "Version needed to extract",
            true,
        ),
        field(
            "flags",
            8,
            zip_flags_type(),
            "General purpose bit flag",
            true,
        ),
        field(
            "compression",
            10,
            compression_type(),
            "Compression method",
            true,
        ),
        field(
            "modified_at",
            12,
            FieldType::custom_display(
                4,
                time::format_dos_datetime(entry.mod_time, entry.mod_date),
                time::encode_dos_datetime_le,
            ),
            "Last modification timestamp decoded from ZIP DOS time/date",
            true,
        ),
        field(
            "mod_time",
            12,
            FieldType::U16Le,
            "Last modification time",
            true,
        ),
        field(
            "mod_date",
            14,
            FieldType::U16Le,
            "Last modification date",
            true,
        ),
        field("crc32", 16, FieldType::U32Le, "CRC-32 checksum", false),
        field(
            "compressed_size",
            20,
            FieldType::U32Le,
            "Compressed size, or 0xffffffff when ZIP64 extra carries it",
            true,
        ),
        field(
            "uncompressed_size",
            24,
            FieldType::U32Le,
            "Uncompressed size, or 0xffffffff when ZIP64 extra carries it",
            true,
        ),
        field(
            "filename_len",
            28,
            FieldType::U16Le,
            "Filename length",
            true,
        ),
        field(
            "extra_len",
            30,
            FieldType::U16Le,
            "Extra field length",
            true,
        ),
        field(
            "comment_len",
            32,
            FieldType::U16Le,
            "File comment length",
            true,
        ),
        field(
            "disk_start",
            34,
            FieldType::U16Le,
            "Disk number where the local header starts",
            false,
        ),
        field(
            "internal_attrs",
            36,
            FieldType::U16Le,
            "Internal file attributes",
            true,
        ),
        field(
            "external_attrs",
            38,
            FieldType::U32Le,
            "External file attributes",
            true,
        ),
        field(
            "local_header_offset",
            42,
            FieldType::U32Le,
            "Relative offset of local file header, or 0xffffffff when ZIP64 extra carries it",
            false,
        ),
    ];

    if entry.filename_len > 0 && entry.filename_len <= 256 {
        fields.push(field(
            "filename",
            CENTRAL_FIXED_LEN,
            FieldType::Utf8(entry.filename_len as usize),
            "Filename from the central directory",
            false,
        ));
    }
    if entry.extra_len > 0 {
        fields.push(field(
            "extra",
            CENTRAL_FIXED_LEN + entry.filename_len as u64,
            FieldType::DataRange(entry.extra_len as u64),
            "Central directory extra fields",
            false,
        ));
    }
    if entry.comment_len > 0 {
        fields.push(field(
            "comment",
            CENTRAL_FIXED_LEN + entry.filename_len as u64 + entry.extra_len as u64,
            FieldType::Utf8(entry.comment_len as usize),
            "Central directory file comment",
            false,
        ));
    }
    add_zip64_extra_fields(
        &mut fields,
        CENTRAL_FIXED_LEN + entry.filename_len as u64,
        &entry.zip64_layout,
    );

    StructDef {
        name: format!(
            "Central Directory Entry {}: {}",
            entry.index,
            entry.display_name()
        ),
        base_offset: entry.offset,
        fields,
        children: vec![],
    }
}

fn build_data_descriptor_struct(
    doc: &mut Document,
    entry: &CentralEntry,
    descriptor_offset: u64,
) -> Option<StructDef> {
    let has_signature = read_bytes_raw(doc, descriptor_offset, 4)
        .is_some_and(|sig| sig == ZIP_DATA_DESCRIPTOR_MAGIC);
    let size_width = if entry.uses_zip64_sizes() { 8 } else { 4 };
    let body_offset = if has_signature { 4 } else { 0 };
    let total_len = body_offset + 4 + size_width * 2;
    if descriptor_offset.checked_add(total_len as u64)? > doc.len() {
        return None;
    }

    let mut fields = Vec::new();
    if has_signature {
        fields.push(field(
            "signature",
            0,
            FieldType::Bytes(4),
            "Optional data descriptor signature",
            false,
        ));
    }
    fields.push(field(
        "crc32",
        body_offset as u64,
        FieldType::U32Le,
        "CRC-32 checksum from the data descriptor",
        false,
    ));
    fields.push(field(
        "compressed_size",
        (body_offset + 4) as u64,
        descriptor_size_type(size_width),
        "Compressed size from the data descriptor",
        false,
    ));
    fields.push(field(
        "uncompressed_size",
        (body_offset + 4 + size_width) as u64,
        descriptor_size_type(size_width),
        "Uncompressed size from the data descriptor",
        false,
    ));

    Some(StructDef {
        name: if has_signature {
            format!("Data Descriptor: {}", entry.display_name())
        } else {
            format!("Data Descriptor: {} [no signature]", entry.display_name())
        },
        base_offset: descriptor_offset,
        fields,
        children: vec![],
    })
}

fn build_eocd_struct(eocd: &Eocd) -> StructDef {
    let mut fields = vec![
        field(
            "signature",
            0,
            FieldType::Bytes(4),
            "End of central directory signature",
            false,
        ),
        field(
            "disk_number",
            4,
            FieldType::U16Le,
            "Current disk number",
            false,
        ),
        field(
            "central_directory_disk",
            6,
            FieldType::U16Le,
            "Disk where the central directory starts",
            false,
        ),
        field(
            "disk_entries",
            8,
            FieldType::U16Le,
            "Central directory entries on this disk",
            false,
        ),
        field(
            "total_entries",
            10,
            FieldType::U16Le,
            "Total central directory entries",
            false,
        ),
        field(
            "central_directory_size",
            12,
            FieldType::U32Le,
            "Central directory byte size, or 0xffffffff for ZIP64",
            false,
        ),
        field(
            "central_directory_offset",
            16,
            FieldType::U32Le,
            "Central directory offset, or 0xffffffff for ZIP64",
            false,
        ),
        field(
            "comment_len",
            20,
            FieldType::U16Le,
            "Archive comment length",
            false,
        ),
    ];
    if eocd.comment_len > 0 {
        fields.push(field(
            "comment",
            EOCD_FIXED_LEN,
            FieldType::Utf8(eocd.comment_len as usize),
            "Archive comment",
            false,
        ));
    }

    StructDef {
        name: "End Of Central Directory".into(),
        base_offset: eocd.offset,
        fields,
        children: vec![],
    }
}

fn build_zip64_eocd_struct(record: &Zip64Eocd) -> StructDef {
    let mut fields = vec![
        field(
            "signature",
            0,
            FieldType::Bytes(4),
            "ZIP64 end of central directory signature",
            false,
        ),
        field(
            "size_of_record",
            4,
            FieldType::U64Le,
            "Size of the remaining ZIP64 EOCD record",
            false,
        ),
        field(
            "version_made_by",
            12,
            FieldType::U16Le,
            "Version made by",
            false,
        ),
        field(
            "version_needed",
            14,
            FieldType::U16Le,
            "Version needed to extract",
            false,
        ),
        field(
            "disk_number",
            16,
            FieldType::U32Le,
            "Current disk number",
            false,
        ),
        field(
            "central_directory_disk",
            20,
            FieldType::U32Le,
            "Disk where the central directory starts",
            false,
        ),
        field(
            "disk_entries",
            24,
            FieldType::U64Le,
            "Central directory entries on this disk",
            false,
        ),
        field(
            "total_entries",
            32,
            FieldType::U64Le,
            "Total central directory entries",
            false,
        ),
        field(
            "central_directory_size",
            40,
            FieldType::U64Le,
            "Central directory byte size",
            false,
        ),
        field(
            "central_directory_offset",
            48,
            FieldType::U64Le,
            "Central directory offset",
            false,
        ),
    ];

    let extensible_len = record.size_of_record.saturating_sub(44);
    if extensible_len > 0 {
        fields.push(field(
            "extensible_data",
            ZIP64_EOCD_MIN_LEN,
            FieldType::DataRange(extensible_len),
            "ZIP64 extensible data sector",
            false,
        ));
    }

    StructDef {
        name: "ZIP64 End Of Central Directory".into(),
        base_offset: record.offset,
        fields,
        children: vec![],
    }
}

fn build_zip64_locator_struct(record: &Zip64Eocd) -> StructDef {
    StructDef {
        name: "ZIP64 End Of Central Directory Locator".into(),
        base_offset: record.locator_offset,
        fields: vec![
            field(
                "signature",
                0,
                FieldType::Bytes(4),
                "ZIP64 EOCD locator signature",
                false,
            ),
            field(
                "disk_with_zip64_eocd",
                4,
                FieldType::U32Le,
                "Disk containing the ZIP64 EOCD record",
                false,
            ),
            field(
                "zip64_eocd_offset",
                8,
                FieldType::U64Le,
                "Offset of the ZIP64 EOCD record",
                false,
            ),
            field(
                "total_disks",
                16,
                FieldType::U32Le,
                "Total number of disks",
                false,
            ),
        ],
        children: vec![],
    }
}

fn local_header_fields(header: &LocalHeader) -> Vec<FieldDef> {
    let mut fields =
        vec![
        field(
            "signature",
            0,
            FieldType::Bytes(4),
            "Local file header signature",
            false,
        ),
        field(
            "version_needed",
            4,
            FieldType::U16Le,
            "Version needed to extract",
            true,
        ),
        field("flags", 6, zip_flags_type(), "General purpose bit flag", true),
        field("compression", 8, compression_type(), "Compression method", true),
        field(
            "modified_at",
            10,
            FieldType::custom_display(
                4,
                time::format_dos_datetime(header.mod_time, header.mod_date),
                time::encode_dos_datetime_le,
            ),
            "Last modification timestamp decoded from ZIP DOS time/date",
            true,
        ),
        field("mod_time", 10, FieldType::U16Le, "Last modification time", true),
        field("mod_date", 12, FieldType::U16Le, "Last modification date", true),
        field("crc32", 14, FieldType::U32Le, "CRC-32 checksum", false),
        field(
            "compressed_size",
            18,
            FieldType::U32Le,
            "Compressed size, zero when a data descriptor carries it, or 0xffffffff for ZIP64",
            true,
        ),
        field(
            "uncompressed_size",
            22,
            FieldType::U32Le,
            "Uncompressed size, zero when a data descriptor carries it, or 0xffffffff for ZIP64",
            true,
        ),
        field("filename_len", 26, FieldType::U16Le, "Filename length", true),
        field("extra_len", 28, FieldType::U16Le, "Extra field length", true),
    ];

    if header.filename_len > 0 && header.filename_len <= 256 {
        fields.push(field(
            "filename",
            LOCAL_FIXED_LEN,
            FieldType::Utf8(header.filename_len as usize),
            "Filename from the local file header",
            false,
        ));
    }
    if header.extra_len > 0 {
        fields.push(field(
            "extra",
            LOCAL_FIXED_LEN + header.filename_len as u64,
            FieldType::DataRange(header.extra_len as u64),
            "Local file extra fields",
            false,
        ));
    }
    fields
}

fn add_zip64_extra_fields(
    fields: &mut Vec<FieldDef>,
    extra_offset: u64,
    layout: &Zip64ExtraLayout,
) {
    if let Some(offset) = layout.uncompressed_size_offset {
        fields.push(field(
            "zip64_uncompressed_size",
            extra_offset + offset as u64,
            FieldType::U64Le,
            "ZIP64 uncompressed size from the extra field",
            false,
        ));
    }
    if let Some(offset) = layout.compressed_size_offset {
        fields.push(field(
            "zip64_compressed_size",
            extra_offset + offset as u64,
            FieldType::U64Le,
            "ZIP64 compressed size from the extra field",
            false,
        ));
    }
    if let Some(offset) = layout.local_header_offset_offset {
        fields.push(field(
            "zip64_local_header_offset",
            extra_offset + offset as u64,
            FieldType::U64Le,
            "ZIP64 local header offset from the extra field",
            false,
        ));
    }
    if let Some(offset) = layout.disk_start_offset {
        fields.push(field(
            "zip64_disk_start",
            extra_offset + offset as u64,
            FieldType::U32Le,
            "ZIP64 disk-start value from the extra field",
            false,
        ));
    }
}

fn parse_zip64_extra_layout(
    extra: &[u8],
    need_uncompressed: bool,
    need_compressed: bool,
    need_local_header: bool,
    need_disk_start: bool,
) -> Zip64ExtraLayout {
    let mut cursor = 0_usize;
    while cursor + 4 <= extra.len() {
        let header_id = le_u16(extra, cursor);
        let data_len = le_u16(extra, cursor + 2) as usize;
        let data_start = cursor + 4;
        let Some(data_end) = data_start.checked_add(data_len) else {
            break;
        };
        if data_end > extra.len() {
            break;
        }

        if header_id == ZIP64_EXTRA_ID {
            return zip64_layout_from_data_start(
                data_start,
                data_len,
                need_uncompressed,
                need_compressed,
                need_local_header,
                need_disk_start,
            );
        }
        cursor = data_end;
    }

    Zip64ExtraLayout::default()
}

fn zip64_layout_from_data_start(
    data_start: usize,
    data_len: usize,
    need_uncompressed: bool,
    need_compressed: bool,
    need_local_header: bool,
    need_disk_start: bool,
) -> Zip64ExtraLayout {
    let mut layout = Zip64ExtraLayout::default();
    let mut offset = data_start;
    let data_end = data_start + data_len;

    if need_uncompressed && offset + 8 <= data_end {
        layout.uncompressed_size_offset = Some(offset);
        offset += 8;
    }
    if need_compressed && offset + 8 <= data_end {
        layout.compressed_size_offset = Some(offset);
        offset += 8;
    }
    if need_local_header && offset + 8 <= data_end {
        layout.local_header_offset_offset = Some(offset);
        offset += 8;
    }
    if need_disk_start && offset + 4 <= data_end {
        layout.disk_start_offset = Some(offset);
    }

    layout
}

fn read_filename(doc: &mut Document, offset: u64, len: u16) -> Option<String> {
    if len == 0 || len > 256 {
        return None;
    }
    read_bytes_raw(doc, offset, len as usize).map(|bytes| String::from_utf8_lossy(&bytes).into())
}

fn local_struct_name(entry: &CentralEntry, header: &LocalHeader, truncated: bool) -> String {
    let name = header
        .filename
        .as_deref()
        .unwrap_or_else(|| entry.display_name());
    let mut label = format!("Local File {}: {}", entry.index, name);
    if header.flags & ZIP_DATA_DESCRIPTOR_FLAG != 0 {
        label.push_str(" [data descriptor]");
    }
    if entry.uses_zip64_sizes() {
        label.push_str(" [ZIP64]");
    }
    if truncated {
        label.push_str(" (truncated)");
    }
    label
}

fn descriptor_size_type(width: usize) -> FieldType {
    if width == 8 {
        FieldType::U64Le
    } else {
        FieldType::U32Le
    }
}

fn zip_flags_type() -> FieldType {
    FieldType::custom_flags(
        FieldType::U16Le,
        vec![
            (0x0001, "Encrypted".into()),
            (0x0008, "Data descriptor".into()),
            (0x0800, "UTF-8".into()),
        ],
    )
}

fn compression_type() -> FieldType {
    FieldType::custom_enum(
        FieldType::U16Le,
        vec![(0, "Stored".into()), (8, "Deflated".into())],
    )
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

fn le_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn le_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

fn read_u64_from(bytes: &[u8], offset: usize) -> Option<u64> {
    (offset + 8 <= bytes.len()).then(|| le_u64(bytes, offset))
}

impl CentralEntry {
    fn display_name(&self) -> &str {
        self.filename.as_deref().unwrap_or("<unnamed>")
    }

    fn uses_zip64_sizes(&self) -> bool {
        self.compressed_size_32 == u32::MAX
            || self.uncompressed_size_32 == u32::MAX
            || self.compressed_size > u32::MAX as u64
            || self.uncompressed_size > u32::MAX as u64
    }
}

impl LocalHeader {
    fn display_name(&self, index: usize) -> String {
        self.filename
            .clone()
            .unwrap_or_else(|| format!("entry_{index}"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{detect, detect_with_cap};
    use crate::config::Config;
    use crate::core::document::Document;
    use crate::format::edit::encode_value;
    use crate::format::types::CustomCodec;
    use crate::format::types::FieldType;

    const SAMPLE_DOS_TIME: u16 = 0x4dd4;
    const SAMPLE_DOS_DATE: u16 = 0x58e3;

    fn open_zip(bytes: Vec<u8>) -> Document {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.zip");
        fs::write(&path, bytes).unwrap();
        Document::open(&path, &Config::default()).unwrap()
    }

    fn push_local_header(bytes: &mut Vec<u8>, name: &[u8], flags: u16, data_len: u32) -> usize {
        let offset = bytes.len();
        bytes.extend_from_slice(&super::ZIP_LOCAL_MAGIC);
        bytes.extend_from_slice(&20_u16.to_le_bytes());
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&SAMPLE_DOS_TIME.to_le_bytes());
        bytes.extend_from_slice(&SAMPLE_DOS_DATE.to_le_bytes());
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        if flags & super::ZIP_DATA_DESCRIPTOR_FLAG == 0 {
            bytes.extend_from_slice(&data_len.to_le_bytes());
            bytes.extend_from_slice(&data_len.to_le_bytes());
        } else {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(name);
        offset
    }

    fn push_central_header(
        bytes: &mut Vec<u8>,
        name: &[u8],
        flags: u16,
        data_len: u32,
        local_offset: usize,
    ) {
        bytes.extend_from_slice(&super::ZIP_CENTRAL_MAGIC);
        bytes.extend_from_slice(&20_u16.to_le_bytes());
        bytes.extend_from_slice(&20_u16.to_le_bytes());
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&SAMPLE_DOS_TIME.to_le_bytes());
        bytes.extend_from_slice(&SAMPLE_DOS_DATE.to_le_bytes());
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&(local_offset as u32).to_le_bytes());
        bytes.extend_from_slice(name);
    }

    fn push_eocd(bytes: &mut Vec<u8>, entries: u16, central_offset: usize, central_size: usize) {
        bytes.extend_from_slice(&super::ZIP_EOCD_MAGIC);
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&entries.to_le_bytes());
        bytes.extend_from_slice(&entries.to_le_bytes());
        bytes.extend_from_slice(&(central_size as u32).to_le_bytes());
        bytes.extend_from_slice(&(central_offset as u32).to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
    }

    fn simple_zip_with_descriptor(include_descriptor_signature: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        let name = b"a.txt";
        let local_offset = push_local_header(&mut bytes, name, 0x0008, 4);
        bytes.extend_from_slice(b"data");
        if include_descriptor_signature {
            bytes.extend_from_slice(&super::ZIP_DATA_DESCRIPTOR_MAGIC);
        }
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());

        let central_offset = bytes.len();
        push_central_header(&mut bytes, name, 0x0008, 4, local_offset);
        let central_size = bytes.len() - central_offset;
        push_eocd(&mut bytes, 1, central_offset, central_size);
        bytes
    }

    #[test]
    fn parses_central_directory_eocd_and_data_descriptor() {
        let mut doc = open_zip(simple_zip_with_descriptor(true));
        let def = detect(&mut doc).expect("zip should be detected");

        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Local File 0: a.txt [data descriptor]"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Data Descriptor: a.txt"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Central Directory Entry 0: a.txt"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "End Of Central Directory"));
    }

    #[test]
    fn parses_data_descriptor_without_optional_signature() {
        let mut doc = open_zip(simple_zip_with_descriptor(false));
        let def = detect(&mut doc).expect("zip should be detected");

        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "Data Descriptor: a.txt [no signature]"));
    }

    #[test]
    fn zip_modification_time_is_editable_dos_datetime() {
        let mut doc = open_zip(simple_zip_with_descriptor(true));
        let def = detect(&mut doc).expect("zip should be detected");
        for structure_name in [
            "Local File 0: a.txt [data descriptor]",
            "Central Directory Entry 0: a.txt",
        ] {
            let structure = def
                .structs
                .iter()
                .find(|structure| structure.name == structure_name)
                .expect("zip structure");
            let modified_at = structure
                .fields
                .iter()
                .find(|field| field.name == "modified_at")
                .expect("modified_at field");
            let FieldType::Custom(custom) = &modified_at.field_type else {
                panic!("modified_at should use custom display");
            };
            let CustomCodec::Display { display, .. } = &custom.codec else {
                panic!("modified_at should use display codec");
            };
            assert_eq!(display, "2024-07-03T09:46:40 (DOS local)");
            let expected = [SAMPLE_DOS_TIME.to_le_bytes(), SAMPLE_DOS_DATE.to_le_bytes()].concat();
            assert_eq!(
                encode_value(&modified_at.field_type, display).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn central_directory_cap_reports_more_entries() {
        let mut bytes = Vec::new();
        let first_local = push_local_header(&mut bytes, b"a", 0, 1);
        bytes.push(b'a');
        let second_local = push_local_header(&mut bytes, b"b", 0, 1);
        bytes.push(b'b');

        let central_offset = bytes.len();
        push_central_header(&mut bytes, b"a", 0, 1, first_local);
        push_central_header(&mut bytes, b"b", 0, 1, second_local);
        let central_size = bytes.len() - central_offset;
        push_eocd(&mut bytes, 2, central_offset, central_size);

        let mut doc = open_zip(bytes);
        let def = detect_with_cap(&mut doc, 1).expect("zip should be detected");
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name.contains("more ZIP entries beyond 1")));
    }

    #[test]
    fn parses_zip64_eocd_and_zip64_extra_sizes() {
        let mut bytes = Vec::new();
        let name = b"big.bin";
        let local_offset = bytes.len();
        bytes.extend_from_slice(&super::ZIP_LOCAL_MAGIC);
        bytes.extend_from_slice(&45_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&20_u16.to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&super::ZIP64_EXTRA_ID.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(&3_u64.to_le_bytes());
        bytes.extend_from_slice(&3_u64.to_le_bytes());
        bytes.extend_from_slice(b"big");

        let central_offset = bytes.len();
        bytes.extend_from_slice(&super::ZIP_CENTRAL_MAGIC);
        bytes.extend_from_slice(&45_u16.to_le_bytes());
        bytes.extend_from_slice(&45_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&28_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&super::ZIP64_EXTRA_ID.to_le_bytes());
        bytes.extend_from_slice(&24_u16.to_le_bytes());
        bytes.extend_from_slice(&3_u64.to_le_bytes());
        bytes.extend_from_slice(&3_u64.to_le_bytes());
        bytes.extend_from_slice(&(local_offset as u64).to_le_bytes());
        let central_size = bytes.len() - central_offset;

        let zip64_eocd_offset = bytes.len();
        bytes.extend_from_slice(&super::ZIP64_EOCD_MAGIC);
        bytes.extend_from_slice(&44_u64.to_le_bytes());
        bytes.extend_from_slice(&45_u16.to_le_bytes());
        bytes.extend_from_slice(&45_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u64.to_le_bytes());
        bytes.extend_from_slice(&1_u64.to_le_bytes());
        bytes.extend_from_slice(&(central_size as u64).to_le_bytes());
        bytes.extend_from_slice(&(central_offset as u64).to_le_bytes());
        bytes.extend_from_slice(&super::ZIP64_LOCATOR_MAGIC);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&(zip64_eocd_offset as u64).to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());

        bytes.extend_from_slice(&super::ZIP_EOCD_MAGIC);
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());

        let mut doc = open_zip(bytes);
        let def = detect(&mut doc).expect("zip64 should be detected");
        let central = def
            .structs
            .iter()
            .find(|structure| structure.name == "Central Directory Entry 0: big.bin")
            .expect("central directory entry");

        assert!(central
            .fields
            .iter()
            .any(|field| field.name == "zip64_compressed_size"
                && matches!(field.field_type, FieldType::U64Le)));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "ZIP64 End Of Central Directory"));
        assert!(def
            .structs
            .iter()
            .any(|structure| structure.name == "ZIP64 End Of Central Directory Locator"));
    }

    #[test]
    fn falls_back_to_partial_local_scan_when_eocd_is_missing() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        bytes.extend_from_slice(&20_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0008_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.push(b'a');
        bytes.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        bytes.extend_from_slice(&[0x50, 0x4b, 0x07, 0x08]);
        bytes.extend_from_slice(&[0; 12]);

        let mut doc = open_zip(bytes);
        let def = detect(&mut doc).expect("zip should still be detected");

        assert_eq!(def.structs.len(), 1);
        assert!(def.structs[0].name.contains("partial scan"));
    }
}
