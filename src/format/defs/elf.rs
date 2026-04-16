use crate::core::document::Document;
use crate::format::detect::{read_bytes_raw, read_u8};
use crate::format::types::*;

const ELF_MAGIC: [u8; 4] = [0x7f, 0x45, 0x4c, 0x46];

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

    let header = build_elf_header(is_64, is_le, doc);

    Some(FormatDef {
        name: if is_64 { "ELF64" } else { "ELF32" }.to_string(),
        structs: vec![header],
    })
}

fn read_u16(doc: &mut Document, offset: u64, is_le: bool) -> Option<u16> {
    let b = read_bytes_raw(doc, offset, 2)?;
    Some(if is_le {
        u16::from_le_bytes([b[0], b[1]])
    } else {
        u16::from_be_bytes([b[0], b[1]])
    })
}

fn read_u32(doc: &mut Document, offset: u64, is_le: bool) -> Option<u32> {
    let b = read_bytes_raw(doc, offset, 4)?;
    Some(if is_le {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    })
}

fn read_u64(doc: &mut Document, offset: u64, is_le: bool) -> Option<u64> {
    let b = read_bytes_raw(doc, offset, 8)?;
    Some(if is_le {
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    } else {
        u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    })
}

fn build_elf_header(is_64: bool, is_le: bool, doc: &mut Document) -> StructDef {
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

    let (phoff, phnum, phentsize) = if is_64 {
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

        let phoff = read_u64(doc, 32, is_le).unwrap_or(0);
        let phentsize = read_u16(doc, 54, is_le).unwrap_or(0) as u64;
        let phnum = read_u16(doc, 56, is_le).unwrap_or(0) as usize;
        (phoff, phnum, phentsize)
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

        let phoff = read_u32(doc, 28, is_le).unwrap_or(0) as u64;
        let phentsize = read_u16(doc, 42, is_le).unwrap_or(0) as u64;
        let phnum = read_u16(doc, 44, is_le).unwrap_or(0) as usize;
        (phoff, phnum, phentsize)
    };

    let mut children = Vec::new();

    if phoff > 0 && phentsize > 0 && phnum > 0 && phnum <= 64 {
        for i in 0..phnum {
            let entry_offset = phoff + i as u64 * phentsize;
            if let Some(ph) = build_program_header(entry_offset, is_64, is_le, doc) {
                children.push(ph);
            }
        }
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
        children,
    }
}

fn build_program_header(
    base: u64,
    is_64: bool,
    is_le: bool,
    doc: &mut Document,
) -> Option<StructDef> {
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

    let p_type_val = read_u32(doc, base, is_le)?;

    let type_variants: Vec<(u64, String)> = vec![
        (0, "PT_NULL".to_string()),
        (1, "PT_LOAD".to_string()),
        (2, "PT_DYNAMIC".to_string()),
        (3, "PT_INTERP".to_string()),
        (4, "PT_NOTE".to_string()),
        (5, "PT_SHLIB".to_string()),
        (6, "PT_PHDR".to_string()),
        (7, "PT_TLS".to_string()),
        (0x6474e550, "PT_GNU_EH_FRAME".to_string()),
        (0x6474e551, "PT_GNU_STACK".to_string()),
        (0x6474e552, "PT_GNU_RELRO".to_string()),
        (0x6474e553, "PT_GNU_PROPERTY".to_string()),
    ];

    let type_label = type_variants
        .iter()
        .find(|(v, _)| *v == p_type_val as u64)
        .map(|(_, name)| name.as_str())
        .unwrap_or("UNKNOWN");

    if is_64 {
        let p_filesz = read_u64(doc, base + 32, is_le)?;
        let _p_memsz = read_u64(doc, base + 40, is_le)?;

        Some(StructDef {
            name: format!("Program Header {}: {}", type_label, p_type_val),
            base_offset: base,
            fields: vec![
                FieldDef {
                    name: "p_type".into(),
                    offset: 0,
                    field_type: FieldType::Enum {
                        inner: Box::new(u32_t.clone()),
                        variants: type_variants,
                    },
                    description: "Segment type".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_flags".into(),
                    offset: 4,
                    field_type: FieldType::Flags {
                        inner: Box::new(u32_t.clone()),
                        flags: vec![(4, "R".into()), (2, "W".into()), (1, "X".into())],
                    },
                    description: "Segment flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_offset".into(),
                    offset: 8,
                    field_type: u64_t.clone(),
                    description: "File offset of segment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_vaddr".into(),
                    offset: 16,
                    field_type: u64_t.clone(),
                    description: "Virtual address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_paddr".into(),
                    offset: 24,
                    field_type: u64_t.clone(),
                    description: "Physical address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_filesz".into(),
                    offset: 32,
                    field_type: u64_t.clone(),
                    description: "Size in file".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_memsz".into(),
                    offset: 40,
                    field_type: u64_t.clone(),
                    description: "Size in memory".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_align".into(),
                    offset: 48,
                    field_type: u64_t,
                    description: "Alignment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "segment_data".into(),
                    offset: 0,
                    field_type: FieldType::DataRange(p_filesz),
                    description: "Segment data range in file".into(),
                    editable: false,
                },
            ],
            children: vec![],
        })
    } else {
        let p_filesz = read_u32(doc, base + 16, is_le)?;
        let _p_memsz = read_u32(doc, base + 20, is_le)?;

        Some(StructDef {
            name: format!("Program Header {}: {}", type_label, p_type_val),
            base_offset: base,
            fields: vec![
                FieldDef {
                    name: "p_type".into(),
                    offset: 0,
                    field_type: FieldType::Enum {
                        inner: Box::new(u32_t.clone()),
                        variants: type_variants,
                    },
                    description: "Segment type".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_offset".into(),
                    offset: 4,
                    field_type: u32_t.clone(),
                    description: "File offset of segment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_vaddr".into(),
                    offset: 8,
                    field_type: u32_t.clone(),
                    description: "Virtual address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_paddr".into(),
                    offset: 12,
                    field_type: u32_t.clone(),
                    description: "Physical address".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_filesz".into(),
                    offset: 16,
                    field_type: u32_t.clone(),
                    description: "Size in file".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_memsz".into(),
                    offset: 20,
                    field_type: u32_t.clone(),
                    description: "Size in memory".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_flags".into(),
                    offset: 24,
                    field_type: FieldType::Flags {
                        inner: Box::new(u32_t.clone()),
                        flags: vec![(4, "R".into()), (2, "W".into()), (1, "X".into())],
                    },
                    description: "Segment flags".into(),
                    editable: true,
                },
                FieldDef {
                    name: "p_align".into(),
                    offset: 28,
                    field_type: u32_t,
                    description: "Alignment".into(),
                    editable: true,
                },
                FieldDef {
                    name: "segment_data".into(),
                    offset: 0,
                    field_type: FieldType::DataRange(p_filesz as u64),
                    description: "Segment data range in file".into(),
                    editable: false,
                },
            ],
            children: vec![],
        })
    }
}
