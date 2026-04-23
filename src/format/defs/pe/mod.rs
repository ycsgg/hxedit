use crate::core::document::Document;
use crate::format::detect::{read_bytes_raw, read_u8, DEFAULT_ENTRY_CAP};
use crate::format::types::*;

// DOS Header constants
const DOS_MAGIC: [u8; 2] = [0x4d, 0x5a]; // "MZ"
const DOS_HEADER_SIZE: u64 = 64;

// PE Signature
const PE_SIGNATURE: [u8; 4] = [0x50, 0x45, 0x00, 0x00]; // "PE\0\0"

// COFF Header sizes
const COFF_HEADER_SIZE: u64 = 20;

// Section Header
const SECTION_HEADER_SIZE: u64 = 40;

// Machine types
const MACHINE_I386: u16 = 0x014c;
const MACHINE_AMD64: u16 = 0x8664;
const MACHINE_ARM: u16 = 0x01c0;
const MACHINE_ARM64: u16 = 0xaa64;

// Optional Header magic
const OPT_MAGIC_PE32: u16 = 0x10b;
const OPT_MAGIC_PE32_PLUS: u16 = 0x20b;

// Machine types

/// Machine type enum names
fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x0000 => "Unknown",
        MACHINE_I386 => "i386",
        MACHINE_AMD64 => "AMD64",
        MACHINE_ARM => "ARM",
        MACHINE_ARM64 => "ARM64",
        0x01c2 => "ARM Thumb",
        0x0166 => "MIPS R4000",
        0x0169 => "MIPS R10000",
        0x01a2 => "Hitachi SH3",
        0x01a3 => "Hitachi SH3 DSP",
        0x01a6 => "Hitachi SH4",
        0x01a8 => "Hitachi SH5",
        0x0200 => "Intel Itanium",
        0x5032 => "RISC-V 32-bit",
        0x5064 => "RISC-V 64-bit",
        0x5128 => "RISC-V 128-bit",
        _ => "Unknown",
    }
}

/// COFF characteristics flags
fn coff_characteristics_flags() -> Vec<(u64, String)> {
    vec![
        (0x0001, "RELOCS_STRIPPED".to_string()),
        (0x0002, "EXECUTABLE_IMAGE".to_string()),
        (0x0004, "LINE_NUMS_STRIPPED".to_string()),
        (0x0008, "LOCAL_SYMS_STRIPPED".to_string()),
        (0x0010, "AGGRESSIVE_WS_TRIM".to_string()),
        (0x0020, "LARGE_ADDRESS_AWARE".to_string()),
        (0x0080, "BYTES_REVERSED_LO".to_string()),
        (0x0100, "32BIT_MACHINE".to_string()),
        (0x0200, "DEBUG_STRIPPED".to_string()),
        (0x0400, "REMOVABLE_RUN_FROM_SWAP".to_string()),
        (0x0800, "NET_RUN_FROM_SWAP".to_string()),
        (0x1000, "SYSTEM".to_string()),
        (0x2000, "DLL".to_string()),
        (0x4000, "UP_SYSTEM_ONLY".to_string()),
        (0x8000, "BYTES_REVERSED_HI".to_string()),
    ]
}

/// Section characteristics flags
fn section_characteristics_flags() -> Vec<(u64, String)> {
    vec![
        (0x0000_0008, "NO_PAD".to_string()),
        (0x0000_0020, "CNT_CODE".to_string()),
        (0x0000_0040, "CNT_INITIALIZED_DATA".to_string()),
        (0x0000_0080, "CNT_UNINITIALIZED_DATA".to_string()),
        (0x0000_0100, "LNK_OTHER".to_string()),
        (0x0000_0200, "LNK_INFO".to_string()),
        (0x0000_0800, "LNK_REMOVE".to_string()),
        (0x0000_1000, "LNK_COMDAT".to_string()),
        (0x0000_8000, "GPREL".to_string()),
        (0x0002_0000, "MEM_PURGEABLE".to_string()),
        (0x0004_0000, "MEM_LOCKED".to_string()),
        (0x0008_0000, "MEM_PRELOAD".to_string()),
        (0x0100_0000, "LNK_NRELOC_OVFL".to_string()),
        (0x0200_0000, "DISCARDABLE".to_string()),
        (0x0400_0000, "NOT_CACHED".to_string()),
        (0x0800_0000, "NOT_PAGED".to_string()),
        (0x1000_0000, "SHARED".to_string()),
        (0x2000_0000, "EXECUTE".to_string()),
        (0x4000_0000, "READ".to_string()),
        (0x8000_0000, "WRITE".to_string()),
    ]
}

struct PeParser<'a> {
    doc: &'a mut Document,
    entry_cap: usize,
    pe_offset: u64,
    is_pe32_plus: bool,
}

impl<'a> PeParser<'a> {
    fn new(doc: &'a mut Document, pe_offset: u64, entry_cap: usize) -> Self {
        Self {
            doc,
            entry_cap,
            pe_offset,
            is_pe32_plus: false,
        }
    }

    fn read_u16_le(&mut self, offset: u64) -> Option<u16> {
        let b = read_bytes_raw(self.doc, offset, 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32_le(&mut self, offset: u64) -> Option<u32> {
        let b = read_bytes_raw(self.doc, offset, 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_u64_le(&mut self, offset: u64) -> Option<u64> {
        let b = read_bytes_raw(self.doc, offset, 8)?;
        Some(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn parse(&mut self) -> Option<FormatDef> {
        // Verify PE signature
        let pe_sig = read_bytes_raw(self.doc, self.pe_offset, 4)?;
        if pe_sig != PE_SIGNATURE {
            return None;
        }

        let coff_offset = self.pe_offset + 4;

        // Parse COFF Header
        let coff_fields = self.build_coff_header_fields(coff_offset)?;

        // Get sizes needed for further parsing
        let _machine = self.read_u16_le(coff_offset)?;
        let num_sections = self.read_u16_le(coff_offset + 2)? as usize;
        let opt_header_size = self.read_u16_le(coff_offset + 16)? as u64;
        let _characteristics = self.read_u16_le(coff_offset + 18)?;

        let mut children = Vec::new();

        // Parse Optional Header if present
        if opt_header_size > 0 {
            let opt_offset = coff_offset + COFF_HEADER_SIZE;
            if let Some(opt_magic) = self.read_u16_le(opt_offset) {
                self.is_pe32_plus = opt_magic == OPT_MAGIC_PE32_PLUS;
                if let Some(opt_fields) =
                    self.build_optional_header_fields(opt_offset, opt_header_size)
                {
                    children.push(opt_fields);
                }
            }
        }

        // Parse Section Table
        let section_table_offset = coff_offset + COFF_HEADER_SIZE + opt_header_size;
        if num_sections > 0 {
            let sections = self.build_section_table(section_table_offset, num_sections);
            children.push(sections);
        }

        let format_name = if self.is_pe32_plus { "PE32+" } else { "PE32" };

        Some(FormatDef {
            name: format_name.to_string(),
            structs: vec![StructDef {
                name: "DOS Header".to_string(),
                base_offset: 0,
                fields: self.build_dos_header_fields(),
                children: vec![StructDef {
                    name: "COFF Header".to_string(),
                    base_offset: coff_offset,
                    fields: coff_fields,
                    children,
                }],
            }],
        })
    }

    fn build_dos_header_fields(&self) -> Vec<FieldDef> {
        vec![
            FieldDef {
                name: "e_magic".to_string(),
                offset: 0,
                field_type: FieldType::Bytes(2),
                description: "Magic number (MZ)".to_string(),
                editable: false,
            },
            FieldDef {
                name: "e_cblp".to_string(),
                offset: 2,
                field_type: FieldType::U16Le,
                description: "Bytes on last page of file".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_cp".to_string(),
                offset: 4,
                field_type: FieldType::U16Le,
                description: "Pages in file".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_crlc".to_string(),
                offset: 6,
                field_type: FieldType::U16Le,
                description: "Relocations".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_cparhdr".to_string(),
                offset: 8,
                field_type: FieldType::U16Le,
                description: "Size of header in paragraphs".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_minalloc".to_string(),
                offset: 10,
                field_type: FieldType::U16Le,
                description: "Minimum extra paragraphs needed".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_maxalloc".to_string(),
                offset: 12,
                field_type: FieldType::U16Le,
                description: "Maximum extra paragraphs needed".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_ss".to_string(),
                offset: 14,
                field_type: FieldType::U16Le,
                description: "Initial (relative) SS value".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_sp".to_string(),
                offset: 16,
                field_type: FieldType::U16Le,
                description: "Initial SP value".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_csum".to_string(),
                offset: 18,
                field_type: FieldType::U16Le,
                description: "Checksum".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_ip".to_string(),
                offset: 20,
                field_type: FieldType::U16Le,
                description: "Initial IP value".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_cs".to_string(),
                offset: 22,
                field_type: FieldType::U16Le,
                description: "Initial (relative) CS value".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_lfarlc".to_string(),
                offset: 24,
                field_type: FieldType::U16Le,
                description: "File address of relocation table".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_ovno".to_string(),
                offset: 26,
                field_type: FieldType::U16Le,
                description: "Overlay number".to_string(),
                editable: true,
            },
            // e_res[4] at offset 28-35 (reserved, skip detailed display)
            FieldDef {
                name: "e_oemid".to_string(),
                offset: 36,
                field_type: FieldType::U16Le,
                description: "OEM identifier".to_string(),
                editable: true,
            },
            FieldDef {
                name: "e_oeminfo".to_string(),
                offset: 38,
                field_type: FieldType::U16Le,
                description: "OEM information".to_string(),
                editable: true,
            },
            // e_res2[10] at offset 40-59 (reserved, skip)
            FieldDef {
                name: "e_lfanew".to_string(),
                offset: 60,
                field_type: FieldType::U32Le,
                description: "File address of new exe header (PE signature)".to_string(),
                editable: true,
            },
        ]
    }

    fn build_coff_header_fields(&mut self, offset: u64) -> Option<Vec<FieldDef>> {
        let machine = self.read_u16_le(offset)?;
        let num_sections = self.read_u16_le(offset + 2)?;
        let time_date_stamp = self.read_u32_le(offset + 4)?;
        let ptr_sym_table = self.read_u32_le(offset + 8)?;
        let num_symbols = self.read_u32_le(offset + 12)?;
        let opt_header_size = self.read_u16_le(offset + 16)?;
        let characteristics = self.read_u16_le(offset + 18)?;

        Some(vec![
            FieldDef {
                name: "Machine".to_string(),
                offset: 0,
                field_type: FieldType::Enum {
                    inner: Box::new(FieldType::U16Le),
                    variants: vec![
                        (MACHINE_I386 as u64, "i386".to_string()),
                        (MACHINE_AMD64 as u64, "AMD64".to_string()),
                        (MACHINE_ARM as u64, "ARM".to_string()),
                        (MACHINE_ARM64 as u64, "ARM64".to_string()),
                    ],
                },
                description: format!("Target machine type: {}", machine_name(machine)),
                editable: true,
            },
            FieldDef {
                name: "NumberOfSections".to_string(),
                offset: 2,
                field_type: FieldType::U16Le,
                description: format!("Number of sections: {}", num_sections),
                editable: false,
            },
            FieldDef {
                name: "TimeDateStamp".to_string(),
                offset: 4,
                field_type: FieldType::U32Le,
                description: format!("Time date stamp: 0x{:08x}", time_date_stamp),
                editable: true,
            },
            FieldDef {
                name: "PointerToSymbolTable".to_string(),
                offset: 8,
                field_type: FieldType::U32Le,
                description: format!("Symbol table offset: 0x{:x}", ptr_sym_table),
                editable: false,
            },
            FieldDef {
                name: "NumberOfSymbols".to_string(),
                offset: 12,
                field_type: FieldType::U32Le,
                description: format!("Number of symbols: {}", num_symbols),
                editable: false,
            },
            FieldDef {
                name: "SizeOfOptionalHeader".to_string(),
                offset: 16,
                field_type: FieldType::U16Le,
                description: format!("Size of optional header: {} bytes", opt_header_size),
                editable: false,
            },
            FieldDef {
                name: "Characteristics".to_string(),
                offset: 18,
                field_type: FieldType::Flags {
                    inner: Box::new(FieldType::U16Le),
                    flags: coff_characteristics_flags(),
                },
                description: format!("Image characteristics: 0x{:04x}", characteristics),
                editable: true,
            },
        ])
    }

    fn build_optional_header_fields(&mut self, offset: u64, size: u64) -> Option<StructDef> {
        let _magic = self.read_u16_le(offset)?;
        let major_linker = read_u8(self.doc, offset + 2)?;
        let minor_linker = read_u8(self.doc, offset + 3)?;

        let mut fields = vec![
            FieldDef {
                name: "Magic".to_string(),
                offset: 0,
                field_type: FieldType::Enum {
                    inner: Box::new(FieldType::U16Le),
                    variants: vec![
                        (OPT_MAGIC_PE32 as u64, "PE32".to_string()),
                        (OPT_MAGIC_PE32_PLUS as u64, "PE32+".to_string()),
                    ],
                },
                description: if self.is_pe32_plus {
                    "PE32+ (64-bit)"
                } else {
                    "PE32 (32-bit)"
                }
                .to_string(),
                editable: false,
            },
            FieldDef {
                name: "MajorLinkerVersion".to_string(),
                offset: 2,
                field_type: FieldType::U8,
                description: format!("Major linker version: {}", major_linker),
                editable: false,
            },
            FieldDef {
                name: "MinorLinkerVersion".to_string(),
                offset: 3,
                field_type: FieldType::U8,
                description: format!("Minor linker version: {}", minor_linker),
                editable: false,
            },
        ];

        // Standard fields (PE32 and PE32+ have different layouts)
        if size >= 28 {
            let size_of_code = self.read_u32_le(offset + 4)?;
            let size_of_init_data = self.read_u32_le(offset + 8)?;
            let size_of_uninit_data = self.read_u32_le(offset + 12)?;
            let entry_point = self.read_u32_le(offset + 16)?;
            let base_of_code = self.read_u32_le(offset + 20)?;

            fields.extend(vec![
                FieldDef {
                    name: "SizeOfCode".to_string(),
                    offset: 4,
                    field_type: FieldType::U32Le,
                    description: format!("Size of code section: {} bytes", size_of_code),
                    editable: false,
                },
                FieldDef {
                    name: "SizeOfInitializedData".to_string(),
                    offset: 8,
                    field_type: FieldType::U32Le,
                    description: format!("Size of initialized data: {} bytes", size_of_init_data),
                    editable: false,
                },
                FieldDef {
                    name: "SizeOfUninitializedData".to_string(),
                    offset: 12,
                    field_type: FieldType::U32Le,
                    description: format!(
                        "Size of uninitialized data: {} bytes",
                        size_of_uninit_data
                    ),
                    editable: false,
                },
                FieldDef {
                    name: "AddressOfEntryPoint".to_string(),
                    offset: 16,
                    field_type: FieldType::U32Le,
                    description: format!("Entry point RVA: 0x{:08x}", entry_point),
                    editable: true,
                },
                FieldDef {
                    name: "BaseOfCode".to_string(),
                    offset: 20,
                    field_type: FieldType::U32Le,
                    description: format!("Base of code RVA: 0x{:08x}", base_of_code),
                    editable: true,
                },
            ]);

            // PE32 has BaseOfData, PE32+ does not
            if !self.is_pe32_plus && size >= 32 {
                let base_of_data = self.read_u32_le(offset + 24)?;
                fields.push(FieldDef {
                    name: "BaseOfData".to_string(),
                    offset: 24,
                    field_type: FieldType::U32Le,
                    description: format!("Base of data RVA: 0x{:08x}", base_of_data),
                    editable: true,
                });
            }
        }

        // Windows-specific fields
        let win_offset = if self.is_pe32_plus { 16 } else { 24 };
        if size >= win_offset as u64 + 48 {
            let image_base = if self.is_pe32_plus {
                self.read_u64_le(offset + win_offset as u64)?
            } else {
                self.read_u32_le(offset + win_offset as u64)? as u64
            };
            let section_align = self
                .read_u32_le(offset + win_offset as u64 + if self.is_pe32_plus { 8 } else { 4 })?;
            let file_align = self
                .read_u32_le(offset + win_offset as u64 + if self.is_pe32_plus { 12 } else { 8 })?;

            fields.extend(vec![
                FieldDef {
                    name: "ImageBase".to_string(),
                    offset: win_offset as u64,
                    field_type: if self.is_pe32_plus {
                        FieldType::U64Le
                    } else {
                        FieldType::U32Le
                    },
                    description: format!("Preferred image base: 0x{:016x}", image_base),
                    editable: true,
                },
                FieldDef {
                    name: "SectionAlignment".to_string(),
                    offset: win_offset as u64 + if self.is_pe32_plus { 8 } else { 4 },
                    field_type: FieldType::U32Le,
                    description: format!("Section alignment: {} bytes", section_align),
                    editable: false,
                },
                FieldDef {
                    name: "FileAlignment".to_string(),
                    offset: win_offset as u64 + if self.is_pe32_plus { 12 } else { 8 },
                    field_type: FieldType::U32Le,
                    description: format!("File alignment: {} bytes", file_align),
                    editable: false,
                },
            ]);
        }

        Some(StructDef {
            name: "Optional Header".to_string(),
            base_offset: offset,
            fields,
            children: vec![],
        })
    }

    fn shown_count(&self, total: usize) -> usize {
        total.min(self.entry_cap.max(1))
    }

    fn build_section_table(&mut self, offset: u64, num_sections: usize) -> StructDef {
        let shown = self.shown_count(num_sections);
        let mut children = Vec::with_capacity(shown);

        for i in 0..shown {
            let section_offset = offset + (i as u64 * SECTION_HEADER_SIZE);
            if let Some(section) = self.build_section_header(section_offset, i) {
                children.push(section);
            }
        }

        StructDef {
            name: "Section Table".to_string(),
            base_offset: offset,
            fields: vec![FieldDef {
                name: "Count".to_string(),
                offset: 0,
                field_type: FieldType::U32Le,
                description: if shown < num_sections {
                    format!("{} sections (showing first {})", num_sections, shown)
                } else {
                    format!("{} sections", num_sections)
                },
                editable: false,
            }],
            children,
        }
    }

    fn build_section_header(&mut self, offset: u64, index: usize) -> Option<StructDef> {
        let name_bytes = read_bytes_raw(self.doc, offset, 8)?;
        let name = String::from_utf8_lossy(&name_bytes)
            .trim_end_matches('\0')
            .trim_end_matches(' ')
            .to_string();

        let virtual_size = self.read_u32_le(offset + 8)?;
        let virtual_addr = self.read_u32_le(offset + 12)?;
        let size_raw = self.read_u32_le(offset + 16)?;
        let ptr_raw = self.read_u32_le(offset + 20)?;
        let ptr_relocs = self.read_u32_le(offset + 24)?;
        let ptr_linenos = self.read_u32_le(offset + 28)?;
        let num_relocs = self.read_u16_le(offset + 32)?;
        let num_linenos = self.read_u16_le(offset + 34)?;
        let characteristics = self.read_u32_le(offset + 36)?;

        let section_data_start = ptr_raw as u64;
        let section_data_end = section_data_start.saturating_add(size_raw as u64);

        let mut fields = vec![
            FieldDef {
                name: "Name".to_string(),
                offset: 0,
                field_type: FieldType::Utf8(8),
                description: format!("Section name: {}", name),
                editable: true,
            },
            FieldDef {
                name: "VirtualSize".to_string(),
                offset: 8,
                field_type: FieldType::U32Le,
                description: format!("Virtual size: {} bytes", virtual_size),
                editable: true,
            },
            FieldDef {
                name: "VirtualAddress".to_string(),
                offset: 12,
                field_type: FieldType::U32Le,
                description: format!("Virtual address (RVA): 0x{:08x}", virtual_addr),
                editable: true,
            },
            FieldDef {
                name: "SizeOfRawData".to_string(),
                offset: 16,
                field_type: FieldType::U32Le,
                description: format!("Size of raw data: {} bytes", size_raw),
                editable: false,
            },
            FieldDef {
                name: "PointerToRawData".to_string(),
                offset: 20,
                field_type: FieldType::U32Le,
                description: format!("File pointer to raw data: 0x{:x}", ptr_raw),
                editable: false,
            },
        ];

        // Add section data range if there's actual data
        if size_raw > 0 && ptr_raw > 0 {
            fields.push(FieldDef {
                name: "section_data".to_string(),
                offset: 20,
                field_type: FieldType::DataRange(section_data_end - section_data_start),
                description: format!(
                    "Section data range: 0x{:x} - 0x{:x}",
                    section_data_start,
                    section_data_end - 1
                ),
                editable: false,
            });
        }

        fields.extend(vec![
            FieldDef {
                name: "PointerToRelocations".to_string(),
                offset: 24,
                field_type: FieldType::U32Le,
                description: format!("File pointer to relocations: 0x{:x}", ptr_relocs),
                editable: false,
            },
            FieldDef {
                name: "PointerToLinenumbers".to_string(),
                offset: 28,
                field_type: FieldType::U32Le,
                description: format!("File pointer to line numbers: 0x{:x}", ptr_linenos),
                editable: false,
            },
            FieldDef {
                name: "NumberOfRelocations".to_string(),
                offset: 32,
                field_type: FieldType::U16Le,
                description: format!("Number of relocations: {}", num_relocs),
                editable: false,
            },
            FieldDef {
                name: "NumberOfLinenumbers".to_string(),
                offset: 34,
                field_type: FieldType::U16Le,
                description: format!("Number of line numbers: {}", num_linenos),
                editable: false,
            },
            FieldDef {
                name: "Characteristics".to_string(),
                offset: 36,
                field_type: FieldType::Flags {
                    inner: Box::new(FieldType::U32Le),
                    flags: section_characteristics_flags(),
                },
                description: format!("Section characteristics: 0x{:08x}", characteristics),
                editable: true,
            },
        ]);

        Some(StructDef {
            name: format!(
                "Section {} [{}]",
                index,
                if name.is_empty() { "<unnamed>" } else { &name }
            ),
            base_offset: offset,
            fields,
            children: vec![],
        })
    }
}

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    // Check minimum size for DOS header
    if doc.len() < DOS_HEADER_SIZE {
        return None;
    }

    // Check DOS magic
    let magic = read_bytes_raw(doc, 0, 2)?;
    if magic != DOS_MAGIC {
        return None;
    }

    // Get PE header offset from e_lfanew (offset 60)
    let pe_offset = {
        let bytes = read_bytes_raw(doc, 60, 4)?;
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
    };

    // Validate PE offset is within file
    if pe_offset + 4 + COFF_HEADER_SIZE > doc.len() {
        return None;
    }

    PeParser::new(doc, pe_offset, entry_cap).parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machine_name() {
        assert_eq!(machine_name(MACHINE_I386), "i386");
        assert_eq!(machine_name(MACHINE_AMD64), "AMD64");
        assert_eq!(machine_name(MACHINE_ARM64), "ARM64");
        assert_eq!(machine_name(0x0000), "Unknown");
    }
}
