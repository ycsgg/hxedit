use std::collections::BTreeSet;

use crate::core::document::{ByteSlot, Document};
use crate::error::HxResult;
use crate::format::types::*;

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
        /// Stable-within-a-flatten id assigned by pre-order struct walk.
        /// Used to track collapse state across rebuilds of the row list.
        node_id: usize,
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
/// `collapsed_nodes` lists the `node_id`s whose descendants should be hidden.
/// `node_id` is assigned by pre-order struct walk, so it's stable as long as
/// the struct tree shape does not change across rebuilds.
pub fn flatten(structs: &[StructValue], collapsed_nodes: &BTreeSet<usize>) -> Vec<InspectorRow> {
    let mut rows = Vec::new();
    let mut field_idx: usize = 0;
    let mut node_counter: usize = 0;
    for sv in structs {
        walk(
            sv,
            0,
            &mut rows,
            &mut field_idx,
            &mut node_counter,
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
) -> BTreeSet<usize> {
    fn visit(
        sv: &StructValue,
        depth: usize,
        counter: &mut usize,
        default_collapsed_depth: usize,
        out: &mut BTreeSet<usize>,
    ) {
        let node_id = *counter;
        *counter += 1;
        let has_children = !sv.fields.is_empty() || !sv.children.is_empty();
        if has_children && depth >= default_collapsed_depth {
            out.insert(node_id);
        }
        for child in &sv.children {
            visit(child, depth + 1, counter, default_collapsed_depth, out);
        }
    }
    let mut out = BTreeSet::new();
    let mut counter = 0;
    for sv in structs {
        visit(sv, 0, &mut counter, default_collapsed_depth, &mut out);
    }
    out
}

fn walk(
    sv: &StructValue,
    depth: usize,
    rows: &mut Vec<InspectorRow>,
    field_idx: &mut usize,
    node_counter: &mut usize,
    collapsed_nodes: &BTreeSet<usize>,
) {
    let node_id = *node_counter;
    *node_counter += 1;

    let has_children = !sv.fields.is_empty() || !sv.children.is_empty();
    let collapsed = has_children && collapsed_nodes.contains(&node_id);

    rows.push(InspectorRow::Header {
        name: sv.name.clone(),
        depth,
        node_id,
        collapsed,
        has_children,
    });

    if collapsed {
        // Still bump field_idx and node_counter so that field_index stays
        // consistent with find_field_def's pre-order walk and node_id stays
        // consistent across rebuilds.
        count_skipped(sv, field_idx, node_counter);
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

    for child in &sv.children {
        walk(
            child,
            depth + 1,
            rows,
            field_idx,
            node_counter,
            collapsed_nodes,
        );
    }
}

/// Advance `field_idx` and `node_counter` for a collapsed subtree so that
/// both counters remain consistent with an uncollapsed walk of the same tree.
fn count_skipped(sv: &StructValue, field_idx: &mut usize, node_counter: &mut usize) {
    *field_idx += sv.fields.len();
    for child in &sv.children {
        *node_counter += 1;
        count_skipped(child, field_idx, node_counter);
    }
}

/// Read bytes from the document at the given offset and length.
fn read_bytes(doc: &mut Document, offset: u64, len: usize) -> HxResult<Vec<u8>> {
    let mut buf = Vec::with_capacity(len);
    for i in 0..len {
        match doc.byte_at(offset + i as u64)? {
            ByteSlot::Present(b) => buf.push(b),
            _ => buf.push(0),
        }
    }
    Ok(buf)
}

/// Parse a single field: read raw bytes from the document and format as display string.
fn parse_field(doc: &mut Document, field: &FieldDef, base_offset: u64) -> HxResult<FieldValue> {
    let abs_offset = base_offset + field.offset;
    let (raw_bytes, size, display) = match &field.field_type {
        FieldType::DataRange(len) => {
            let len = *len;
            let end = abs_offset + len;
            let display = if len == 0 {
                "empty".to_owned()
            } else {
                format!("0x{:x}–0x{:x} ({} bytes)", abs_offset, end - 1, len)
            };
            (Vec::new(), len as usize, display)
        }
        _ => {
            let size = field.field_type.byte_size().unwrap_or(0);
            let raw_bytes = read_bytes(doc, abs_offset, size)?;
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
    use super::*;

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
        let mut set = BTreeSet::new();
        set.insert(0); // Header
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
        let mut set = BTreeSet::new();
        set.insert(0);
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
        let mut set = BTreeSet::new();
        set.insert(1); // Child1
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
        assert!(set.contains(&1));
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
    fn flatten_node_id_is_stable_under_reflatten() {
        let tree = sample_tree();
        let first = flatten(&tree, &BTreeSet::new());
        let second = flatten(&tree, &BTreeSet::new());
        let ids_a: Vec<_> = first
            .iter()
            .filter_map(|r| match r {
                InspectorRow::Header { node_id, name, .. } => Some((node_id, name.clone())),
                _ => None,
            })
            .collect();
        let ids_b: Vec<_> = second
            .iter()
            .filter_map(|r| match r {
                InspectorRow::Header { node_id, name, .. } => Some((node_id, name.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(ids_a, ids_b);
    }
}
