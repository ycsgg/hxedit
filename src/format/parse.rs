use std::collections::{BTreeSet, HashMap};

use crate::core::document::Document;
use crate::error::HxResult;
use crate::format::types::*;

/// Stable identifier for a struct inside the parsed tree.
///
/// Each component is `(struct_name, sibling_index_among_same_name)` — the
/// sibling index counts how many earlier siblings of the same parent share
/// the same name, so two `Chunk: IDAT` structs at the same level get distinct
/// paths. This survives structure edits (e.g. adding or removing a chunk)
/// as long as the existing structs keep their name+sibling-rank.
pub type NodePath = Vec<(String, usize)>;

/// A parsed field value.
///
/// Contains definition info, raw bytes read from the file, and formatted display text.
#[derive(Debug, Clone)]
pub struct FieldValue {
    /// The field definition this value corresponds to.
    pub def: FieldDef,
    /// Absolute offset of this field in the file (base_offset + def.offset).
    pub abs_offset: u64,
    /// Raw bytes read from the file (length == def.field_type.byte_size()).
    pub raw_bytes: Vec<u8>,
    /// Formatted display text, e.g. "0x003e (EM_X86_64)".
    pub display: String,
    /// Number of bytes this field occupies.
    pub size: usize,
}

/// A parsed structure block.
#[derive(Debug, Clone)]
pub struct StructValue {
    /// Structure name.
    pub name: String,
    /// Absolute start offset.
    pub base_offset: u64,
    /// Parsed field values.
    pub fields: Vec<FieldValue>,
    /// Parsed child structures.
    pub children: Vec<StructValue>,
}

/// A single row in the inspector panel.
///
/// Flattened from the StructValue tree for rendering.
#[derive(Debug, Clone)]
pub enum InspectorRow {
    /// Structure header line, e.g. "▼ ELF Header".
    Header {
        name: String,
        depth: usize,
        /// Stable identity of this struct within the parsed tree. Used to
        /// track collapse state and to locate the struct after a rebuild
        /// even when earlier siblings have been added or removed.
        node_path: NodePath,
        /// Whether the struct's children are currently hidden.
        collapsed: bool,
        /// Whether this struct has any fields or nested structs to collapse.
        has_children: bool,
    },
    /// Field line.
    Field {
        /// Global index in the StructValue tree, used for locating during edits.
        field_index: usize,
        /// Field name.
        name: String,
        /// Formatted value.
        display: String,
        /// Absolute file offset range [start, start+size).
        abs_offset: u64,
        size: usize,
        /// Indentation depth.
        depth: usize,
        /// Whether this field is editable.
        editable: bool,
    },
}

/// Flatten a StructValue tree into a list of renderable rows.
///
/// `collapsed_nodes` lists the [`NodePath`]s whose descendants should be
/// hidden. Using a path-based key keeps collapse state pinned to a specific
/// struct even when sibling counts change between rebuilds — e.g. a PNG
/// gaining or losing a chunk no longer rolls the collapsed state into an
/// unrelated neighbor.
pub fn flatten(structs: &[StructValue], collapsed_nodes: &BTreeSet<NodePath>) -> Vec<InspectorRow> {
    let mut rows = Vec::new();
    let mut field_idx: usize = 0;
    let mut name_counts: HashMap<String, usize> = HashMap::new();
    let parent_path: NodePath = Vec::new();
    for sv in structs {
        let sibling_index = next_sibling_index(&mut name_counts, &sv.name);
        walk(
            sv,
            0,
            &parent_path,
            sibling_index,
            &mut rows,
            &mut field_idx,
            collapsed_nodes,
        );
    }
    rows
}

/// Compute the initial collapsed-nodes set for a freshly parsed tree.
///
/// Every struct at `depth >= default_collapsed_depth` is marked collapsed.
/// Structs without any fields or children are skipped — collapsing them would
/// be a no-op and would only confuse later navigation.
pub fn initial_collapsed_nodes(
    structs: &[StructValue],
    default_collapsed_depth: usize,
) -> BTreeSet<NodePath> {
    fn visit(
        sv: &StructValue,
        depth: usize,
        parent_path: &NodePath,
        sibling_index: usize,
        default_collapsed_depth: usize,
        out: &mut BTreeSet<NodePath>,
    ) {
        let mut path = parent_path.clone();
        path.push((sv.name.clone(), sibling_index));
        let has_children = !sv.fields.is_empty() || !sv.children.is_empty();
        if has_children && depth >= default_collapsed_depth {
            out.insert(path.clone());
        }
        let mut name_counts: HashMap<String, usize> = HashMap::new();
        for child in &sv.children {
            let child_idx = next_sibling_index(&mut name_counts, &child.name);
            visit(
                child,
                depth + 1,
                &path,
                child_idx,
                default_collapsed_depth,
                out,
            );
        }
    }
    let mut out = BTreeSet::new();
    let mut name_counts: HashMap<String, usize> = HashMap::new();
    let root: NodePath = Vec::new();
    for sv in structs {
        let idx = next_sibling_index(&mut name_counts, &sv.name);
        visit(sv, 0, &root, idx, default_collapsed_depth, &mut out);
    }
    out
}

/// Return the next sibling index for a given name, incrementing the counter
/// so the following sibling with the same name gets the next slot.
fn next_sibling_index(counts: &mut HashMap<String, usize>, name: &str) -> usize {
    let entry = counts.entry(name.to_owned()).or_insert(0);
    let idx = *entry;
    *entry += 1;
    idx
}

fn walk(
    sv: &StructValue,
    depth: usize,
    parent_path: &NodePath,
    sibling_index: usize,
    rows: &mut Vec<InspectorRow>,
    field_idx: &mut usize,
    collapsed_nodes: &BTreeSet<NodePath>,
) {
    let mut node_path = parent_path.clone();
    node_path.push((sv.name.clone(), sibling_index));

    let has_children = !sv.fields.is_empty() || !sv.children.is_empty();
    let collapsed = has_children && collapsed_nodes.contains(&node_path);

    rows.push(InspectorRow::Header {
        name: sv.name.clone(),
        depth,
        node_path: node_path.clone(),
        collapsed,
        has_children,
    });

    if collapsed {
        // Still bump field_idx so field_index stays consistent with
        // find_field_def's pre-order walk even when nodes are collapsed.
        count_skipped_fields(sv, field_idx);
        return;
    }

    for fv in &sv.fields {
        rows.push(InspectorRow::Field {
            field_index: *field_idx,
            name: fv.def.name.clone(),
            display: fv.display.clone(),
            abs_offset: fv.abs_offset,
            size: fv.size,
            depth: depth + 1,
            editable: fv.def.editable,
        });
        *field_idx += 1;
    }

    let mut name_counts: HashMap<String, usize> = HashMap::new();
    for child in &sv.children {
        let child_idx = next_sibling_index(&mut name_counts, &child.name);
        walk(
            child,
            depth + 1,
            &node_path,
            child_idx,
            rows,
            field_idx,
            collapsed_nodes,
        );
    }
}

/// Advance `field_idx` for a collapsed subtree so that indices remain
/// consistent with an uncollapsed walk of the same tree.
fn count_skipped_fields(sv: &StructValue, field_idx: &mut usize) {
    *field_idx += sv.fields.len();
    for child in &sv.children {
        count_skipped_fields(child, field_idx);
    }
}

/// Read bytes from the document at the given offset and length.
///
/// Uses a batched piece-walking read instead of per-byte `byte_at` to keep
/// parse cost proportional to the number of fields, not bytes. Short reads
/// (past EOF or partial page) are padded with `0x00` to preserve the legacy
/// fallback behavior that `byte_at`-based parsers relied on.
fn read_bytes(doc: &mut Document, offset: u64, len: usize) -> HxResult<Vec<u8>> {
    let mut buf = doc.read_logical_range(offset, len)?;
    if buf.len() < len {
        buf.resize(len, 0);
    }
    Ok(buf)
}

/// Parse a single field: read raw bytes from the document and format as display string.
fn parse_field(doc: &mut Document, field: &FieldDef, base_offset: u64) -> HxResult<FieldValue> {
    let abs_offset = base_offset + field.offset;
    let doc_len = doc.len();
    let (raw_bytes, size, display) = match &field.field_type {
        FieldType::DataRange(len) => {
            let len = *len;
            let end = abs_offset + len;
            // Saturate to usize so the reported `size` stays within addressable
            // range on 32-bit platforms; note in display when that happened so
            // the inspector doesn't silently round down a multi-GiB chunk.
            let size = usize::try_from(len).unwrap_or(usize::MAX);
            let overflow = (size as u64) < len;
            let display = if len == 0 {
                "empty".to_owned()
            } else if overflow {
                format!(
                    "0x{:x}–0x{:x} ({} bytes) (overflow)",
                    abs_offset,
                    end - 1,
                    len
                )
            } else {
                format!("0x{:x}–0x{:x} ({} bytes)", abs_offset, end - 1, len)
            };
            (Vec::new(), size, display)
        }
        _ => {
            let declared_size = field.field_type.byte_size().unwrap_or(0);
            // Clamp the reported size to what's actually available in the
            // document. `read_bytes` pads with zeros so `raw_bytes.len()` is
            // always `declared_size`, but if we report that to the edit path
            // it will happily try to write past EOF for fields that straddle
            // the end of file.
            let available = doc_len.saturating_sub(abs_offset);
            let size = declared_size.min(available as usize);
            let raw_bytes = read_bytes(doc, abs_offset, declared_size)?;
            let display = format_value(&field.field_type, &raw_bytes);
            (raw_bytes, size, display)
        }
    };
    Ok(FieldValue {
        def: field.clone(),
        abs_offset,
        raw_bytes,
        display,
        size,
    })
}

/// Parse a complete format definition, producing a StructValue tree.
pub fn parse_format(def: &FormatDef, doc: &mut Document) -> HxResult<Vec<StructValue>> {
    def.structs.iter().map(|sd| parse_struct(doc, sd)).collect()
}

fn parse_struct(doc: &mut Document, sd: &StructDef) -> HxResult<StructValue> {
    let fields: Vec<FieldValue> = sd
        .fields
        .iter()
        .map(|fd| parse_field(doc, fd, sd.base_offset))
        .collect::<HxResult<_>>()?;
    let children: Vec<StructValue> = sd
        .children
        .iter()
        .map(|child| parse_struct(doc, child))
        .collect::<HxResult<_>>()?;
    Ok(StructValue {
        name: sd.name.clone(),
        base_offset: sd.base_offset,
        fields,
        children,
    })
}

/// Decode raw bytes as an unsigned u64 value based on the field type.
pub fn decode_unsigned(field_type: &FieldType, raw: &[u8]) -> u64 {
    match field_type {
        FieldType::U8 => raw.first().copied().unwrap_or(0) as u64,
        FieldType::U16Le => u16::from_le_bytes(
            raw.get(..2)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 2]),
        ) as u64,
        FieldType::U16Be => u16::from_be_bytes(
            raw.get(..2)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 2]),
        ) as u64,
        FieldType::U32Le => u32::from_le_bytes(
            raw.get(..4)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 4]),
        ) as u64,
        FieldType::U32Be => u32::from_be_bytes(
            raw.get(..4)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 4]),
        ) as u64,
        FieldType::U64Le => u64::from_le_bytes(
            raw.get(..8)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 8]),
        ),
        FieldType::U64Be => u64::from_be_bytes(
            raw.get(..8)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0; 8]),
        ),
        _ => 0,
    }
}

/// Format raw bytes according to FieldType into a human-readable string.
pub fn format_value(field_type: &FieldType, raw: &[u8]) -> String {
    match field_type {
        FieldType::U8 => format!("0x{:02x}", raw.first().copied().unwrap_or(0)),
        FieldType::U16Le => {
            let v = u16::from_le_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("0x{:04x}", v)
        }
        FieldType::U16Be => {
            let v = u16::from_be_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("0x{:04x}", v)
        }
        FieldType::U32Le => {
            let v = u32::from_le_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("0x{:08x}", v)
        }
        FieldType::U32Be => {
            let v = u32::from_be_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("0x{:08x}", v)
        }
        FieldType::U64Le => {
            let v = u64::from_le_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("0x{:016x}", v)
        }
        FieldType::U64Be => {
            let v = u64::from_be_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("0x{:016x}", v)
        }
        FieldType::I8 => format!("{}", raw.first().copied().unwrap_or(0) as i8),
        FieldType::I16Le => {
            let v = i16::from_le_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("{}", v)
        }
        FieldType::I16Be => {
            let v = i16::from_be_bytes(
                raw.get(..2)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 2]),
            );
            format!("{}", v)
        }
        FieldType::I32Le => {
            let v = i32::from_le_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("{}", v)
        }
        FieldType::I32Be => {
            let v = i32::from_be_bytes(
                raw.get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 4]),
            );
            format!("{}", v)
        }
        FieldType::I64Le => {
            let v = i64::from_le_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("{}", v)
        }
        FieldType::I64Be => {
            let v = i64::from_be_bytes(
                raw.get(..8)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([0; 8]),
            );
            format!("{}", v)
        }
        FieldType::Bytes(n) => raw
            .iter()
            .take(*n)
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" "),
        FieldType::Utf8(n) => {
            let s = String::from_utf8_lossy(&raw[..*n.min(&raw.len())]);
            format!("\"{}\"", s.trim_end_matches('\0'))
        }
        FieldType::DataRange(len) => {
            if *len == 0 {
                "empty".to_owned()
            } else {
                format!("{} bytes", len)
            }
        }
        FieldType::Enum { inner, variants } => {
            let base = format_value(inner, raw);
            let numeric = decode_unsigned(inner, raw);
            let label = variants
                .iter()
                .find(|(v, _)| *v == numeric)
                .map(|(_, name)| name.as_str())
                .unwrap_or("?");
            format!("{} ({})", base, label)
        }
        FieldType::Flags { inner, flags } => {
            let base = format_value(inner, raw);
            let numeric = decode_unsigned(inner, raw);
            let active: Vec<&str> = flags
                .iter()
                .filter(|(bit, _)| numeric & bit != 0)
                .map(|(_, name)| name.as_str())
                .collect();
            if active.is_empty() {
                base
            } else {
                format!("{} [{}]", base, active.join(" | "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::config::Config;
    use crate::core::document::Document;

    fn field(name: &str, offset: u64) -> FieldValue {
        FieldValue {
            def: FieldDef {
                name: name.to_owned(),
                offset,
                field_type: FieldType::U8,
                description: String::new(),
                editable: true,
            },
            abs_offset: offset,
            raw_bytes: vec![0],
            display: "0x00".to_owned(),
            size: 1,
        }
    }

    fn sample_tree() -> Vec<StructValue> {
        // Header
        //   ├─ f0
        //   ├─ f1
        //   └─ Child1
        //        ├─ c0
        //        └─ c1
        // Second
        //   └─ s0
        vec![
            StructValue {
                name: "Header".into(),
                base_offset: 0,
                fields: vec![field("f0", 0), field("f1", 1)],
                children: vec![StructValue {
                    name: "Child1".into(),
                    base_offset: 2,
                    fields: vec![field("c0", 2), field("c1", 3)],
                    children: vec![],
                }],
            },
            StructValue {
                name: "Second".into(),
                base_offset: 4,
                fields: vec![field("s0", 4)],
                children: vec![],
            },
        ]
    }

    #[test]
    fn flatten_with_empty_collapsed_set_expands_all() {
        let tree = sample_tree();
        let rows = flatten(&tree, &BTreeSet::new());
        // 3 headers + 5 fields = 8 rows
        assert_eq!(rows.len(), 8);
        assert!(matches!(
            rows[0],
            InspectorRow::Header { ref name, collapsed: false, has_children: true, .. } if name == "Header"
        ));
        assert!(matches!(
            rows[3],
            InspectorRow::Header { ref name, collapsed: false, has_children: true, .. } if name == "Child1"
        ));
        assert!(matches!(
            rows[6],
            InspectorRow::Header { ref name, collapsed: false, has_children: true, .. } if name == "Second"
        ));
    }

    #[test]
    fn flatten_with_collapsed_root_hides_children() {
        let tree = sample_tree();
        let mut set: BTreeSet<NodePath> = BTreeSet::new();
        set.insert(vec![("Header".into(), 0)]);
        let rows = flatten(&tree, &set);
        // Header (collapsed, no visible fields) + Second + s0 = 3 rows
        assert_eq!(rows.len(), 3);
        assert!(matches!(
            rows[0],
            InspectorRow::Header { ref name, collapsed: true, has_children: true, .. } if name == "Header"
        ));
        assert!(matches!(
            rows[1],
            InspectorRow::Header { ref name, collapsed: false, .. } if name == "Second"
        ));
    }

    #[test]
    fn flatten_preserves_field_index_across_collapsed_nodes() {
        let tree = sample_tree();
        // Collapse only the first top-level; Second's field should still be index 4.
        let mut set: BTreeSet<NodePath> = BTreeSet::new();
        set.insert(vec![("Header".into(), 0)]);
        let rows = flatten(&tree, &set);
        let InspectorRow::Field { field_index, .. } = rows
            .iter()
            .find(|r| matches!(r, InspectorRow::Field { name, .. } if name == "s0"))
            .cloned()
            .unwrap()
        else {
            panic!("expected field");
        };
        assert_eq!(field_index, 4);
    }

    #[test]
    fn flatten_collapses_nested_child_independently() {
        let tree = sample_tree();
        let mut set: BTreeSet<NodePath> = BTreeSet::new();
        set.insert(vec![("Header".into(), 0), ("Child1".into(), 0)]);
        let rows = flatten(&tree, &set);
        // Header + f0 + f1 + Child1 (collapsed) + Second + s0 = 6
        assert_eq!(rows.len(), 6);
        let child1_idx = rows
            .iter()
            .position(|r| matches!(r, InspectorRow::Header { name, .. } if name == "Child1"))
            .unwrap();
        assert!(matches!(
            rows[child1_idx],
            InspectorRow::Header {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn initial_collapsed_nodes_hides_deep_structs() {
        let tree = sample_tree();
        let set = initial_collapsed_nodes(&tree, 1);
        // depth 1 = Child1 only
        assert_eq!(set.len(), 1);
        assert!(set.contains(&vec![("Header".into(), 0), ("Child1".into(), 0)]));
    }

    #[test]
    fn initial_collapsed_nodes_never_collapses_childless_struct() {
        let tree = vec![StructValue {
            name: "Empty".into(),
            base_offset: 0,
            fields: vec![],
            children: vec![],
        }];
        let set = initial_collapsed_nodes(&tree, 0);
        assert!(set.is_empty());
    }

    #[test]
    fn flatten_node_path_is_stable_under_reflatten() {
        let tree = sample_tree();
        let first = flatten(&tree, &BTreeSet::new());
        let second = flatten(&tree, &BTreeSet::new());
        let paths_a: Vec<_> = first
            .iter()
            .filter_map(|r| match r {
                InspectorRow::Header {
                    node_path, name, ..
                } => Some((node_path.clone(), name.clone())),
                _ => None,
            })
            .collect();
        let paths_b: Vec<_> = second
            .iter()
            .filter_map(|r| match r {
                InspectorRow::Header {
                    node_path, name, ..
                } => Some((node_path.clone(), name.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(paths_a, paths_b);
    }

    #[test]
    fn collapse_survives_sibling_insertion_at_earlier_position() {
        // Baseline: A, B(collapsed), C. User collapsed B by path.
        let base = vec![
            StructValue {
                name: "A".into(),
                base_offset: 0,
                fields: vec![field("a0", 0)],
                children: vec![],
            },
            StructValue {
                name: "B".into(),
                base_offset: 1,
                fields: vec![field("b0", 1)],
                children: vec![],
            },
            StructValue {
                name: "C".into(),
                base_offset: 2,
                fields: vec![field("c0", 2)],
                children: vec![],
            },
        ];
        let mut collapsed: BTreeSet<NodePath> = BTreeSet::new();
        collapsed.insert(vec![("B".into(), 0)]);
        let rows_before = flatten(&base, &collapsed);
        let b_before = rows_before
            .iter()
            .find(|r| matches!(r, InspectorRow::Header { name, .. } if name == "B"))
            .unwrap();
        assert!(matches!(
            b_before,
            InspectorRow::Header {
                collapsed: true,
                ..
            }
        ));

        // Now prepend a new struct before B. B's position shifts but its path
        // (still "B" at sibling index 0) should keep it collapsed.
        let after = vec![
            StructValue {
                name: "NEW".into(),
                base_offset: 0,
                fields: vec![field("n0", 0)],
                children: vec![],
            },
            base[0].clone(),
            base[1].clone(),
            base[2].clone(),
        ];
        let rows_after = flatten(&after, &collapsed);
        let b_after = rows_after
            .iter()
            .find(|r| matches!(r, InspectorRow::Header { name, .. } if name == "B"))
            .unwrap();
        assert!(matches!(
            b_after,
            InspectorRow::Header {
                collapsed: true,
                ..
            }
        ));
        // And the new struct should NOT pick up the stale "id=1" collapse.
        let new_struct = rows_after
            .iter()
            .find(|r| matches!(r, InspectorRow::Header { name, .. } if name == "NEW"))
            .unwrap();
        assert!(matches!(
            new_struct,
            InspectorRow::Header {
                collapsed: false,
                ..
            }
        ));
    }

    #[test]
    fn same_named_siblings_get_distinct_paths() {
        let tree = vec![
            StructValue {
                name: "Chunk".into(),
                base_offset: 0,
                fields: vec![field("a", 0)],
                children: vec![],
            },
            StructValue {
                name: "Chunk".into(),
                base_offset: 1,
                fields: vec![field("b", 1)],
                children: vec![],
            },
        ];
        let rows = flatten(&tree, &BTreeSet::new());
        let paths: Vec<NodePath> = rows
            .iter()
            .filter_map(|r| match r {
                InspectorRow::Header { node_path, .. } => Some(node_path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 2);
        assert_ne!(paths[0], paths[1]);
    }

    fn doc_with_bytes(bytes: &[u8]) -> Document {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blob.bin");
        fs::write(&path, bytes).unwrap();
        let doc = Document::open(&path, &Config::default()).unwrap();
        // Leak the tempdir — tests are short-lived and the file stays alive
        // via the still-open Document handle until the process exits.
        std::mem::forget(dir);
        doc
    }

    #[test]
    fn parse_field_clamps_size_when_declared_bytes_overflow_eof() {
        // Document has only 2 bytes, but the field type declares 4.
        let mut doc = doc_with_bytes(&[0x61, 0x62]);
        let field = FieldDef {
            name: "tail".into(),
            offset: 1,
            field_type: FieldType::Utf8(4),
            description: String::new(),
            editable: true,
        };
        let fv = parse_field(&mut doc, &field, 0).unwrap();
        // Only 1 byte actually available past offset 1.
        assert_eq!(fv.size, 1);
        // raw_bytes still padded to 4 for display purposes.
        assert_eq!(fv.raw_bytes.len(), 4);
    }

    #[test]
    fn parse_field_size_matches_declared_when_bytes_available() {
        let mut doc = doc_with_bytes(&[0x61, 0x62, 0x63, 0x64, 0x65]);
        let field = FieldDef {
            name: "word".into(),
            offset: 0,
            field_type: FieldType::U32Le,
            description: String::new(),
            editable: true,
        };
        let fv = parse_field(&mut doc, &field, 0).unwrap();
        assert_eq!(fv.size, 4);
    }

    #[test]
    fn parse_field_data_range_saturates_size_and_marks_overflow() {
        // DataRange declaring a length that overflows usize on 32-bit must
        // saturate and annotate the display so the UI doesn't silently lie.
        let mut doc = doc_with_bytes(&[0u8; 16]);
        let huge = (usize::MAX as u64).saturating_add(1);
        let field = FieldDef {
            name: "chunk".into(),
            offset: 0,
            field_type: FieldType::DataRange(huge),
            description: String::new(),
            editable: false,
        };
        let fv = parse_field(&mut doc, &field, 0).unwrap();
        if huge > usize::MAX as u64 {
            assert_eq!(fv.size, usize::MAX);
            assert!(fv.display.contains("(overflow)"), "got: {}", fv.display);
        } else {
            // On 64-bit platforms `huge` still fits (usize::MAX+1 saturates back
            // to u64::MAX-ish only when usize is 64-bit); at minimum the display
            // should be well-formed.
            assert!(fv.display.contains("bytes"));
        }
    }

    #[test]
    fn parse_field_data_range_no_overflow_marker_for_small_len() {
        let mut doc = doc_with_bytes(&[0u8; 16]);
        let field = FieldDef {
            name: "chunk".into(),
            offset: 0,
            field_type: FieldType::DataRange(8),
            description: String::new(),
            editable: false,
        };
        let fv = parse_field(&mut doc, &field, 0).unwrap();
        assert_eq!(fv.size, 8);
        assert!(!fv.display.contains("(overflow)"));
    }
}
