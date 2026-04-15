use crate::core::document::Document;
use crate::format::detect::{read_bytes_raw, read_u8};
use crate::format::types::*;

/// ELF magic bytes: 0x7f 'E' 'L' 'F'
const ELF_MAGIC: [u8; 4] = [0x7f, 0x45, 0x4c, 0x46];

/// Detect and parse ELF format.
///
/// Reads the first 4 bytes to check magic, then determines 32/64-bit from ei_class.
pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    if doc.len() < 16 {
        return None;
    }

    let magic = read_bytes_raw(doc, 0, 4)?;
    if magic != ELF_MAGIC {
        return None;
    }

    let ei_class = read_u8(doc, 4)?;
    let ei_data = read_u8(doc, 5)?;

    let is_64 = ei_class == 2;
    let is_le = ei_data == 1;

    let header = build_elf_header(is_64, is_le);

    Some(FormatDef {
        name: if is_64 { "ELF64" } else { "ELF32" }.to_string(),
        structs: vec![header],
    })
}

fn build_elf_header(is_64: bool, is_le: bool) -> StructDef {
    let u16_t = if is_le {
        FieldType::U16Le
    } else {
        FieldType::U16Be
    };
    let u32_t = if is_le {
        FieldType::U32Le
    } else {
        FieldType::U32Be
    };
    let u64_t = if is_le {
        FieldType::U64Le
    } else {
        FieldType::U64Be
    };
    let addr_t = if is_64 { u64_t.clone() } else { u32_t.clone() };
    let off_t = addr_t.clone();

    let mut fields = vec![
        FieldDef {
            name: "e_ident".into(),
            offset: 0,
            field_type: FieldType::Bytes(16),
            description: "ELF identification".into(),
            editable: false,
        },
        FieldDef {
            name: "ei_class".into(),
            offset: 4,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: vec![(1, "32-bit".into()), (2, "64-bit".into())],
            },
            description: "File class".into(),
            editable: true,
        },
        FieldDef {
            name: "ei_data".into(),
            offset: 5,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: vec![(1, "Little-endian".into()), (2, "Big-endian".into())],
            },
            description: "Data encoding".into(),
            editable: true,
        },
        FieldDef {
            name: "ei_version".into(),
            offset: 6,
            field_type: FieldType::U8,
            description: "ELF version".into(),
            editable: true,
        },
        FieldDef {
            name: "ei_osabi".into(),
            offset: 7,
            field_type: FieldType::Enum {
                inner: Box::new(FieldType::U8),
                variants: vec![(0, "ELFOSABI_NONE".into()), (3, "ELFOSABI_LINUX".into())],
            },
            description: "OS/ABI identification".into(),
            editable: true,
        },
        FieldDef {
            name: "e_type".into(),
            offset: 16,
            field_type: FieldType::Enum {
                inner: Box::new(u16_t.clone()),
                variants: vec![
                    (0, "ET_NONE".into()),
                    (1, "ET_REL".into()),
                    (2, "ET_EXEC".into()),
                    (3, "ET_DYN".into()),
                    (4, "ET_CORE".into()),
                ],
            },
            description: "Object file type".into(),
            editable: true,
        },
        FieldDef {
            name: "e_machine".into(),
            offset: 18,
            field_type: FieldType::Enum {
                inner: Box::new(u16_t.clone()),
                variants: vec![
                    (0x03, "EM_386".into()),
                    (0x3e, "EM_X86_64".into()),
                    (0xb7, "EM_AARCH64".into()),
                    (0xf3, "EM_RISCV".into()),
                ],
            },
            description: "Architecture".into(),
            editable: true,
        },
        FieldDef {
            name: "e_version".into(),
            offset: 20,
            field_type: u32_t.clone(),
            description: "Object file version".into(),
            editable: true,
        },
    ];

    if is_64 {
        fields.extend(vec![
            FieldDef {
                name: "e_entry".into(),
                offset: 24,
                field_type: addr_t.clone(),
                description: "Entry point virtual address".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phoff".into(),
                offset: 32,
                field_type: off_t.clone(),
                description: "Program header table offset".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shoff".into(),
                offset: 40,
                field_type: off_t.clone(),
                description: "Section header table offset".into(),
                editable: true,
            },
            FieldDef {
                name: "e_flags".into(),
                offset: 48,
                field_type: u32_t.clone(),
                description: "Processor-specific flags".into(),
                editable: true,
            },
            FieldDef {
                name: "e_ehsize".into(),
                offset: 52,
                field_type: u16_t.clone(),
                description: "ELF header size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phentsize".into(),
                offset: 54,
                field_type: u16_t.clone(),
                description: "Program header entry size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phnum".into(),
                offset: 56,
                field_type: u16_t.clone(),
                description: "Number of program headers".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shentsize".into(),
                offset: 58,
                field_type: u16_t.clone(),
                description: "Section header entry size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shnum".into(),
                offset: 60,
                field_type: u16_t.clone(),
                description: "Number of section headers".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shstrndx".into(),
                offset: 62,
                field_type: u16_t.clone(),
                description: "Section name string table index".into(),
                editable: true,
            },
        ]);
    } else {
        fields.extend(vec![
            FieldDef {
                name: "e_entry".into(),
                offset: 24,
                field_type: addr_t.clone(),
                description: "Entry point virtual address".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phoff".into(),
                offset: 28,
                field_type: off_t.clone(),
                description: "Program header table offset".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shoff".into(),
                offset: 32,
                field_type: off_t.clone(),
                description: "Section header table offset".into(),
                editable: true,
            },
            FieldDef {
                name: "e_flags".into(),
                offset: 36,
                field_type: u32_t.clone(),
                description: "Processor-specific flags".into(),
                editable: true,
            },
            FieldDef {
                name: "e_ehsize".into(),
                offset: 40,
                field_type: u16_t.clone(),
                description: "ELF header size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phentsize".into(),
                offset: 42,
                field_type: u16_t.clone(),
                description: "Program header entry size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_phnum".into(),
                offset: 44,
                field_type: u16_t.clone(),
                description: "Number of program headers".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shentsize".into(),
                offset: 46,
                field_type: u16_t.clone(),
                description: "Section header entry size".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shnum".into(),
                offset: 48,
                field_type: u16_t.clone(),
                description: "Number of section headers".into(),
                editable: true,
            },
            FieldDef {
                name: "e_shstrndx".into(),
                offset: 50,
                field_type: u16_t.clone(),
                description: "Section name string table index".into(),
                editable: true,
            },
        ]);
    }

    StructDef {
        name: if is_64 {
            "ELF64 Header"
        } else {
            "ELF32 Header"
        }
        .to_string(),
        base_offset: 0,
        fields,
        children: vec![],
    }
}
