/// A field's data type.
///
/// Describes how to decode raw bytes into a readable value.
/// Both Lua scripts and Rust built-in definitions map to these variants.
#[derive(Debug, Clone)]
pub enum FieldType {
    U8,
    U16Le,
    U16Be,
    U32Le,
    U32Be,
    U64Le,
    U64Be,
    I8,
    I16Le,
    I16Be,
    I32Le,
    I32Be,
    I64Le,
    I64Be,
    /// Fixed-length byte string, e.g. magic bytes (displayed as hex).
    Bytes(usize),
    /// Fixed-length UTF-8 string.
    Utf8(usize),
    /// Variable-length data range displayed as "start–end (N bytes)".
    /// The length is computed at detect time and stored here.
    DataRange(u64),
    /// Custom field display and edit encoding.
    ///
    /// Formats can use this for values whose user-facing representation spans
    /// or transforms multiple on-disk fields, such as a PCAP timestamp made
    /// from `ts_sec` plus `ts_usec`. Built-in enum/flag decorations can also
    /// be expressed through this path so display and edit conversion share one
    /// inspector pipeline.
    Custom(CustomField),
}

impl FieldType {
    pub fn custom_display(
        bytes: usize,
        display: impl Into<String>,
        encode: fn(&str) -> Result<Vec<u8>, String>,
    ) -> Self {
        Self::Custom(CustomField {
            bytes,
            codec: CustomCodec::Display {
                display: display.into(),
                encode,
            },
        })
    }

    pub fn custom_enum(inner: FieldType, variants: Vec<(u64, String)>) -> Self {
        Self::Custom(CustomField {
            bytes: inner.byte_size().unwrap_or(0),
            codec: CustomCodec::Enum {
                inner: Box::new(inner),
                variants,
            },
        })
    }

    pub fn custom_flags(inner: FieldType, flags: Vec<(u64, String)>) -> Self {
        Self::Custom(CustomField {
            bytes: inner.byte_size().unwrap_or(0),
            codec: CustomCodec::Flags {
                inner: Box::new(inner),
                flags,
            },
        })
    }

    /// Returns the number of bytes this field type consumes.
    pub fn byte_size(&self) -> Option<usize> {
        match self {
            Self::U8 | Self::I8 => Some(1),
            Self::U16Le | Self::U16Be | Self::I16Le | Self::I16Be => Some(2),
            Self::U32Le | Self::U32Be | Self::I32Le | Self::I32Be => Some(4),
            Self::U64Le | Self::U64Be | Self::I64Le | Self::I64Be => Some(8),
            Self::Bytes(n) | Self::Utf8(n) => Some(*n),
            Self::DataRange(_) => None,
            Self::Custom(custom) => Some(custom.bytes),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CustomField {
    pub bytes: usize,
    pub codec: CustomCodec,
}

#[derive(Debug, Clone)]
pub enum CustomCodec {
    Display {
        display: String,
        encode: fn(&str) -> Result<Vec<u8>, String>,
    },
    Enum {
        inner: Box<FieldType>,
        variants: Vec<(u64, String)>,
    },
    Flags {
        inner: Box<FieldType>,
        flags: Vec<(u64, String)>,
    },
}

/// Definition of a single field within a struct.
#[derive(Debug, Clone)]
pub struct FieldDef {
    /// Field name, e.g. "e_type".
    pub name: String,
    /// Offset relative to the owning StructDef's base_offset.
    pub offset: u64,
    /// Data type of this field.
    pub field_type: FieldType,
    /// Human-readable description shown in tooltip / status bar.
    pub description: String,
    /// Whether this field can be edited via the inspector page.
    /// Magic bytes etc. are typically set to false.
    pub editable: bool,
}

/// A structure block (header / section / chunk).
///
/// Recursive: can contain nested child structures.
/// E.g. ELF Header contains Program Header Table children.
#[derive(Debug, Clone)]
pub struct StructDef {
    /// Structure name, e.g. "ELF Header".
    pub name: String,
    /// Absolute start offset of this structure in the file.
    pub base_offset: u64,
    /// Fields within this structure.
    pub fields: Vec<FieldDef>,
    /// Nested child structures.
    pub children: Vec<StructDef>,
}

/// A complete format definition.
///
/// Contains the format name and top-level structure list.
/// Returned by a detector after identifying the format.
pub struct FormatDef {
    /// Format name, e.g. "ELF", "PNG", "ZIP".
    pub name: String,
    /// Top-level structures.
    pub structs: Vec<StructDef>,
}
