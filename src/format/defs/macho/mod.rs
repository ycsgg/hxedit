//! Mach-O format support (macOS/iOS executables)
//!
//! Supports both regular Mach-O and Fat (universal) Mach-O files.

use crate::core::document::Document;
use crate::format::detect::{read_bytes_raw, DEFAULT_ENTRY_CAP};
use crate::format::types::*;

// Mach-O magic numbers
const MH_MAGIC: u32 = 0xFEED_FACE; // 32-bit, big-endian
const MH_CIGAM: u32 = 0xCEFA_EDFE; // 32-bit, little-endian
const MH_MAGIC_64: u32 = 0xFEED_FACF; // 64-bit, big-endian
const MH_CIGAM_64: u32 = 0xCFFA_EDFE; // 64-bit, little-endian
const FAT_MAGIC: u32 = 0xCAFE_BABE; // Fat binary, big-endian
const FAT_CIGAM: u32 = 0xBEBA_FECA; // Fat binary, little-endian

// Header sizes
const MACH_HEADER_SIZE: u64 = 28; // 32-bit Mach-O header
const MACH_HEADER_64_SIZE: u64 = 32; // 64-bit Mach-O header
const FAT_HEADER_SIZE: u64 = 8; // Fat header (magic + nfat_arch)
const FAT_ARCH_SIZE: u64 = 20; // 32-bit fat_arch

// Load command types
const LC_SEGMENT: u32 = 0x1;
const LC_SYMTAB: u32 = 0x2;
const LC_SYMSEG: u32 = 0x3;
const LC_THREAD: u32 = 0x4;
const LC_UNIXTHREAD: u32 = 0x5;
const LC_LOADFVMLIB: u32 = 0x6;
const LC_IDFVMLIB: u32 = 0x7;
const LC_IDENT: u32 = 0x8;
const LC_FVMFILE: u32 = 0x9;
const LC_PREPAGE: u32 = 0xa;
const LC_DYSYMTAB: u32 = 0xb;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_ID_DYLIB: u32 = 0xd;
const LC_LOAD_DYLINKER: u32 = 0xe;
const LC_ID_DYLINKER: u32 = 0xf;
const LC_PREBOUND_DYLIB: u32 = 0x10;
const LC_ROUTINES: u32 = 0x11;
const LC_SUB_FRAMEWORK: u32 = 0x12;
const LC_SUB_UMBRELLA: u32 = 0x13;
const LC_SUB_CLIENT: u32 = 0x14;
const LC_SUB_LIBRARY: u32 = 0x15;
const LC_TWOLEVEL_HINTS: u32 = 0x16;
const LC_PREBIND_CKSUM: u32 = 0x17;
const LC_LOAD_WEAK_DYLIB: u32 = 0x80000018;
const LC_SEGMENT_64: u32 = 0x19;
const LC_ROUTINES_64: u32 = 0x1a;
const LC_UUID: u32 = 0x1b;
const LC_RPATH: u32 = 0x8000001c;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const LC_SEGMENT_SPLIT_INFO: u32 = 0x1e;
const LC_REEXPORT_DYLIB: u32 = 0x8000001f;
const LC_LAZY_LOAD_DYLIB: u32 = 0x20;
const LC_ENCRYPTION_INFO: u32 = 0x21;
const LC_DYLD_INFO: u32 = 0x22;
const LC_DYLD_INFO_ONLY: u32 = 0x80000022;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x80000023;
const LC_VERSION_MIN_MACOSX: u32 = 0x24;
const LC_VERSION_MIN_IPHONEOS: u32 = 0x25;
const LC_FUNCTION_STARTS: u32 = 0x26;
const LC_DYLD_ENVIRONMENT: u32 = 0x27;
const LC_MAIN: u32 = 0x80000028;
const LC_DATA_IN_CODE: u32 = 0x29;
const LC_SOURCE_VERSION: u32 = 0x2A;
const LC_DYLIB_CODE_SIGN_DRS: u32 = 0x2B;
const LC_ENCRYPTION_INFO_64: u32 = 0x2C;
const LC_LINKER_OPTION: u32 = 0x2D;
const LC_LINKER_OPTIMIZATION_HINT: u32 = 0x2E;
const LC_VERSION_MIN_TVOS: u32 = 0x2F;
const LC_VERSION_MIN_WATCHOS: u32 = 0x30;
const LC_NOTE: u32 = 0x31;
const LC_BUILD_VERSION: u32 = 0x32;
const LC_DYLD_EXPORTS_TRIE: u32 = 0x80000033;
const LC_DYLD_CHAINED_FIXUPS: u32 = 0x80000034;

// CPU types
const CPU_TYPE_X86: u32 = 7;
const CPU_TYPE_X86_64: u32 = 0x01000007;
const CPU_TYPE_ARM: u32 = 12;
const CPU_TYPE_ARM64: u32 = 0x0100000C;
const CPU_TYPE_POWERPC: u32 = 18;
const CPU_TYPE_POWERPC64: u32 = 0x01000012;

// File types
const MH_OBJECT: u32 = 1;
const MH_EXECUTE: u32 = 2;
const MH_FVMLIB: u32 = 3;
const MH_CORE: u32 = 4;
const MH_PRELOAD: u32 = 5;
const MH_DYLIB: u32 = 6;
const MH_DYLINKER: u32 = 7;
const MH_BUNDLE: u32 = 8;
const MH_DYLIB_STUB: u32 = 9;
const MH_DSYM: u32 = 10;
const MH_KEXT_BUNDLE: u32 = 11;

fn cpu_type_name(cpu_type: u32) -> &'static str {
    match cpu_type {
        CPU_TYPE_X86 => "x86",
        CPU_TYPE_X86_64 => "x86_64",
        CPU_TYPE_ARM => "ARM",
        CPU_TYPE_ARM64 => "ARM64",
        CPU_TYPE_POWERPC => "PowerPC",
        CPU_TYPE_POWERPC64 => "PowerPC64",
        _ => "Unknown",
    }
}

fn file_type_name(filetype: u32) -> &'static str {
    match filetype {
        MH_OBJECT => "Object",
        MH_EXECUTE => "Executable",
        MH_FVMLIB => "FVMLib",
        MH_CORE => "Core",
        MH_PRELOAD => "Preload",
        MH_DYLIB => "Dylib",
        MH_DYLINKER => "Dylinker",
        MH_BUNDLE => "Bundle",
        MH_DYLIB_STUB => "Dylib Stub",
        MH_DSYM => "dSYM",
        MH_KEXT_BUNDLE => "Kext Bundle",
        _ => "Unknown",
    }
}

fn load_command_name(cmd: u32) -> &'static str {
    match cmd {
        LC_SEGMENT => "LC_SEGMENT",
        LC_SYMTAB => "LC_SYMTAB",
        LC_SYMSEG => "LC_SYMSEG",
        LC_THREAD => "LC_THREAD",
        LC_UNIXTHREAD => "LC_UNIXTHREAD",
        LC_LOADFVMLIB => "LC_LOADFVMLIB",
        LC_IDFVMLIB => "LC_IDFVMLIB",
        LC_IDENT => "LC_IDENT",
        LC_FVMFILE => "LC_FVMFILE",
        LC_PREPAGE => "LC_PREPAGE",
        LC_DYSYMTAB => "LC_DYSYMTAB",
        LC_LOAD_DYLIB => "LC_LOAD_DYLIB",
        LC_ID_DYLIB => "LC_ID_DYLIB",
        LC_LOAD_DYLINKER => "LC_LOAD_DYLINKER",
        LC_ID_DYLINKER => "LC_ID_DYLINKER",
        LC_PREBOUND_DYLIB => "LC_PREBOUND_DYLIB",
        LC_ROUTINES => "LC_ROUTINES",
        LC_SUB_FRAMEWORK => "LC_SUB_FRAMEWORK",
        LC_SUB_UMBRELLA => "LC_SUB_UMBRELLA",
        LC_SUB_CLIENT => "LC_SUB_CLIENT",
        LC_SUB_LIBRARY => "LC_SUB_LIBRARY",
        LC_TWOLEVEL_HINTS => "LC_TWOLEVEL_HINTS",
        LC_PREBIND_CKSUM => "LC_PREBIND_CKSUM",
        LC_LOAD_WEAK_DYLIB => "LC_LOAD_WEAK_DYLIB",
        LC_SEGMENT_64 => "LC_SEGMENT_64",
        LC_ROUTINES_64 => "LC_ROUTINES_64",
        LC_UUID => "LC_UUID",
        LC_RPATH => "LC_RPATH",
        LC_CODE_SIGNATURE => "LC_CODE_SIGNATURE",
        LC_SEGMENT_SPLIT_INFO => "LC_SEGMENT_SPLIT_INFO",
        LC_REEXPORT_DYLIB => "LC_REEXPORT_DYLIB",
        LC_LAZY_LOAD_DYLIB => "LC_LAZY_LOAD_DYLIB",
        LC_ENCRYPTION_INFO => "LC_ENCRYPTION_INFO",
        LC_DYLD_INFO => "LC_DYLD_INFO",
        LC_DYLD_INFO_ONLY => "LC_DYLD_INFO_ONLY",
        LC_LOAD_UPWARD_DYLIB => "LC_LOAD_UPWARD_DYLIB",
        LC_VERSION_MIN_MACOSX => "LC_VERSION_MIN_MACOSX",
        LC_VERSION_MIN_IPHONEOS => "LC_VERSION_MIN_IPHONEOS",
        LC_FUNCTION_STARTS => "LC_FUNCTION_STARTS",
        LC_DYLD_ENVIRONMENT => "LC_DYLD_ENVIRONMENT",
        LC_MAIN => "LC_MAIN",
        LC_DATA_IN_CODE => "LC_DATA_IN_CODE",
        LC_SOURCE_VERSION => "LC_SOURCE_VERSION",
        LC_DYLIB_CODE_SIGN_DRS => "LC_DYLIB_CODE_SIGN_DRS",
        LC_ENCRYPTION_INFO_64 => "LC_ENCRYPTION_INFO_64",
        LC_LINKER_OPTION => "LC_LINKER_OPTION",
        LC_LINKER_OPTIMIZATION_HINT => "LC_LINKER_OPTIMIZATION_HINT",
        LC_VERSION_MIN_TVOS => "LC_VERSION_MIN_TVOS",
        LC_VERSION_MIN_WATCHOS => "LC_VERSION_MIN_WATCHOS",
        LC_NOTE => "LC_NOTE",
        LC_BUILD_VERSION => "LC_BUILD_VERSION",
        LC_DYLD_EXPORTS_TRIE => "LC_DYLD_EXPORTS_TRIE",
        LC_DYLD_CHAINED_FIXUPS => "LC_DYLD_CHAINED_FIXUPS",
        _ => "Unknown",
    }
}

struct MachoParser<'a> {
    doc: &'a mut Document,
    entry_cap: usize,
    is_64: bool,
    is_le: bool,
    base_offset: u64,
}

impl<'a> MachoParser<'a> {
    fn new(doc: &'a mut Document, is_64: bool, is_le: bool, entry_cap: usize) -> Self {
        Self {
            doc,
            entry_cap,
            is_64,
            is_le,
            base_offset: 0,
        }
    }

    fn read_u32(&mut self, offset: u64) -> Option<u32> {
        let b = read_bytes_raw(self.doc, self.base_offset + offset, 4)?;
        Some(if self.is_le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    }

    fn read_u64(&mut self, offset: u64) -> Option<u64> {
        let b = read_bytes_raw(self.doc, self.base_offset + offset, 8)?;
        Some(if self.is_le {
            u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        } else {
            u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        })
    }

    fn u32_t(&self) -> FieldType {
        if self.is_le {
            FieldType::U32Le
        } else {
            FieldType::U32Be
        }
    }

    fn u64_t(&self) -> FieldType {
        if self.is_le {
            FieldType::U64Le
        } else {
            FieldType::U64Be
        }
    }

    fn parse(&mut self) -> Option<FormatDef> {
        let header_size = if self.is_64 {
            MACH_HEADER_64_SIZE
        } else {
            MACH_HEADER_SIZE
        };

        if self.doc.len() < self.base_offset + header_size {
            return None;
        }

        let header_fields = self.build_mach_header_fields()?;
        let ncmds = self.read_u32(16)? as usize;
        let sizeofcmds = self.read_u32(20)?;

        let mut children = Vec::new();

        // Parse load commands
        if ncmds > 0 && sizeofcmds > 0 {
            let cmds_offset = header_size;
            let load_commands = self.parse_load_commands(cmds_offset, ncmds, sizeofcmds);
            if !load_commands.children.is_empty() {
                children.push(load_commands);
            }
        }

        let format_name = if self.is_64 {
            "Mach-O 64-bit"
        } else {
            "Mach-O 32-bit"
        }
        .to_string();

        Some(FormatDef {
            name: format_name,
            structs: vec![StructDef {
                name: "Mach Header".to_string(),
                base_offset: self.base_offset,
                fields: header_fields,
                children,
            }],
        })
    }

    fn build_mach_header_fields(&mut self) -> Option<Vec<FieldDef>> {
        let _magic = self.read_u32(0)?;
        let cputype = self.read_u32(4)?;
        let cpusubtype = self.read_u32(8)?;
        let filetype = self.read_u32(12)?;
        let ncmds = self.read_u32(16)?;
        let sizeofcmds = self.read_u32(20)?;
        let flags = self.read_u32(24)?;

        let mut fields = vec![
            FieldDef {
                name: "magic".to_string(),
                offset: 0,
                field_type: FieldType::U32Le, // Will display based on actual value
                description: if self.is_64 {
                    if self.is_le {
                        "MH_CIGAM_64 (64-bit LE)"
                    } else {
                        "MH_MAGIC_64 (64-bit BE)"
                    }
                } else {
                    if self.is_le {
                        "MH_CIGAM (32-bit LE)"
                    } else {
                        "MH_MAGIC (32-bit BE)"
                    }
                }
                .to_string(),
                editable: false,
            },
            FieldDef {
                name: "cputype".to_string(),
                offset: 4,
                field_type: FieldType::Enum {
                    inner: Box::new(self.u32_t()),
                    variants: vec![
                        (CPU_TYPE_X86 as u64, "x86".to_string()),
                        (CPU_TYPE_X86_64 as u64, "x86_64".to_string()),
                        (CPU_TYPE_ARM as u64, "ARM".to_string()),
                        (CPU_TYPE_ARM64 as u64, "ARM64".to_string()),
                        (CPU_TYPE_POWERPC as u64, "PowerPC".to_string()),
                        (CPU_TYPE_POWERPC64 as u64, "PowerPC64".to_string()),
                    ],
                },
                description: format!("CPU type: {}", cpu_type_name(cputype)),
                editable: false,
            },
            FieldDef {
                name: "cpusubtype".to_string(),
                offset: 8,
                field_type: self.u32_t(),
                description: format!("CPU subtype: 0x{:x}", cpusubtype),
                editable: true,
            },
            FieldDef {
                name: "filetype".to_string(),
                offset: 12,
                field_type: FieldType::Enum {
                    inner: Box::new(self.u32_t()),
                    variants: vec![
                        (MH_OBJECT as u64, "Object".to_string()),
                        (MH_EXECUTE as u64, "Executable".to_string()),
                        (MH_FVMLIB as u64, "FVMLib".to_string()),
                        (MH_CORE as u64, "Core".to_string()),
                        (MH_PRELOAD as u64, "Preload".to_string()),
                        (MH_DYLIB as u64, "Dylib".to_string()),
                        (MH_DYLINKER as u64, "Dylinker".to_string()),
                        (MH_BUNDLE as u64, "Bundle".to_string()),
                        (MH_DSYM as u64, "dSYM".to_string()),
                    ],
                },
                description: format!("File type: {}", file_type_name(filetype)),
                editable: false,
            },
            FieldDef {
                name: "ncmds".to_string(),
                offset: 16,
                field_type: self.u32_t(),
                description: format!("Number of load commands: {}", ncmds),
                editable: false,
            },
            FieldDef {
                name: "sizeofcmds".to_string(),
                offset: 20,
                field_type: self.u32_t(),
                description: format!("Size of load commands: {} bytes", sizeofcmds),
                editable: false,
            },
            FieldDef {
                name: "flags".to_string(),
                offset: 24,
                field_type: self.u32_t(),
                description: format!("Flags: 0x{:08x}", flags),
                editable: true,
            },
        ];

        // 64-bit has reserved field
        if self.is_64 {
            fields.push(FieldDef {
                name: "reserved".to_string(),
                offset: 28,
                field_type: self.u32_t(),
                description: "Reserved".to_string(),
                editable: false,
            });
        }

        Some(fields)
    }

    fn shown_count(&self, total: usize) -> usize {
        total.min(self.entry_cap.max(1))
    }

    fn parse_load_commands(&mut self, offset: u64, ncmds: usize, sizeofcmds: u32) -> StructDef {
        let shown = self.shown_count(ncmds);
        let mut children = Vec::with_capacity(shown);

        let mut cmd_offset = offset;
        let cmds_end = offset + sizeofcmds as u64;

        for i in 0..shown {
            if cmd_offset + 8 > cmds_end || self.base_offset + cmd_offset + 8 > self.doc.len() {
                break;
            }

            if let Some((cmd, cmdsize)) = self.read_load_cmd_header(cmd_offset) {
                if let Some(cmd_struct) = self.build_load_command(cmd_offset, i, cmd, cmdsize) {
                    children.push(cmd_struct);
                }
                cmd_offset += cmdsize as u64;
            } else {
                break;
            }
        }

        StructDef {
            name: "Load Commands".to_string(),
            base_offset: self.base_offset + offset,
            fields: vec![FieldDef {
                name: "Count".to_string(),
                offset: 0,
                field_type: self.u32_t(),
                description: if shown < ncmds {
                    format!("{} load commands (showing first {})", ncmds, shown)
                } else {
                    format!("{} load commands", ncmds)
                },
                editable: false,
            }],
            children,
        }
    }

    fn read_load_cmd_header(&mut self, offset: u64) -> Option<(u32, u32)> {
        let cmd = self.read_u32(offset)?;
        let cmdsize = self.read_u32(offset + 4)?;
        Some((cmd, cmdsize))
    }

    fn build_load_command(
        &mut self,
        offset: u64,
        index: usize,
        cmd: u32,
        cmdsize: u32,
    ) -> Option<StructDef> {
        let cmd_name = load_command_name(cmd);

        let mut fields = vec![
            FieldDef {
                name: "cmd".to_string(),
                offset: 0,
                field_type: FieldType::Enum {
                    inner: Box::new(self.u32_t()),
                    variants: vec![
                        (LC_SEGMENT as u64, "LC_SEGMENT".to_string()),
                        (LC_SEGMENT_64 as u64, "LC_SEGMENT_64".to_string()),
                        (LC_SYMTAB as u64, "LC_SYMTAB".to_string()),
                        (LC_DYSYMTAB as u64, "LC_DYSYMTAB".to_string()),
                        (LC_LOAD_DYLIB as u64, "LC_LOAD_DYLIB".to_string()),
                        (LC_ID_DYLIB as u64, "LC_ID_DYLIB".to_string()),
                        (LC_LOAD_DYLINKER as u64, "LC_LOAD_DYLINKER".to_string()),
                        (LC_ID_DYLINKER as u64, "LC_ID_DYLINKER".to_string()),
                        (LC_MAIN as u64, "LC_MAIN".to_string()),
                        (LC_UUID as u64, "LC_UUID".to_string()),
                        (LC_CODE_SIGNATURE as u64, "LC_CODE_SIGNATURE".to_string()),
                    ],
                },
                description: format!("Command: {}", cmd_name),
                editable: false,
            },
            FieldDef {
                name: "cmdsize".to_string(),
                offset: 4,
                field_type: self.u32_t(),
                description: format!("Command size: {} bytes", cmdsize),
                editable: false,
            },
        ];

        let mut children = Vec::new();

        // Parse segment-specific fields
        let is_segment = cmd == LC_SEGMENT || cmd == LC_SEGMENT_64;
        if is_segment {
            if let Some(segment) = self.parse_segment(offset, cmd == LC_SEGMENT_64) {
                children = segment;
            }
        }

        // Parse LC_MAIN for entry point
        if cmd == LC_MAIN && cmdsize >= 24 {
            let entryoff = self.read_u64(offset + 8)?;
            let stacksize = self.read_u64(offset + 16)?;
            fields.extend(vec![
                FieldDef {
                    name: "entryoff".to_string(),
                    offset: 8,
                    field_type: self.u64_t(),
                    description: format!("Entry point offset: 0x{:x}", entryoff),
                    editable: false,
                },
                FieldDef {
                    name: "stacksize".to_string(),
                    offset: 16,
                    field_type: self.u64_t(),
                    description: format!("Initial stack size: {}", stacksize),
                    editable: false,
                },
            ]);
        }

        // Parse LC_UUID
        if cmd == LC_UUID && cmdsize >= 24 {
            let uuid_bytes = read_bytes_raw(self.doc, self.base_offset + offset + 8, 16)?;
            let uuid_str = format!(
                "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                uuid_bytes[0], uuid_bytes[1], uuid_bytes[2], uuid_bytes[3],
                uuid_bytes[4], uuid_bytes[5],
                uuid_bytes[6], uuid_bytes[7],
                uuid_bytes[8], uuid_bytes[9],
                uuid_bytes[10], uuid_bytes[11], uuid_bytes[12], uuid_bytes[13], uuid_bytes[14], uuid_bytes[15]
            );
            fields.push(FieldDef {
                name: "uuid".to_string(),
                offset: 8,
                field_type: FieldType::Bytes(16),
                description: format!("UUID: {}", uuid_str),
                editable: false,
            });
        }

        Some(StructDef {
            name: format!("{} [{}]", cmd_name, index),
            base_offset: self.base_offset + offset,
            fields,
            children,
        })
    }

    fn parse_segment(&mut self, offset: u64, is_64: bool) -> Option<Vec<StructDef>> {
        let segname_bytes = read_bytes_raw(self.doc, self.base_offset + offset + 8, 16)?;
        let _segname = String::from_utf8_lossy(&segname_bytes)
            .trim_end_matches('\0')
            .trim_end_matches(' ')
            .to_string();

        let (_vmaddr, _vmsize, fileoff, filesize, _maxprot, _initprot, nsects, _flags) = if is_64 {
            (
                self.read_u64(offset + 24)?,
                self.read_u64(offset + 32)?,
                self.read_u64(offset + 40)?,
                self.read_u64(offset + 48)?,
                self.read_u32(offset + 56)?,
                self.read_u32(offset + 60)?,
                self.read_u32(offset + 64)?,
                self.read_u32(offset + 68)?,
            )
        } else {
            (
                self.read_u32(offset + 24)? as u64,
                self.read_u32(offset + 28)? as u64,
                self.read_u32(offset + 32)? as u64,
                self.read_u32(offset + 36)? as u64,
                self.read_u32(offset + 40)?,
                self.read_u32(offset + 44)?,
                self.read_u32(offset + 48)?,
                self.read_u32(offset + 52)?,
            )
        };

        let mut children = Vec::new();

        // Add segment data range if there's file data
        if filesize > 0 {
            let seg_data_start = fileoff;
            let seg_data_end = fileoff + filesize;
            children.push(StructDef {
                name: "Segment Data".to_string(),
                base_offset: fileoff,
                fields: vec![FieldDef {
                    name: "segment_data".to_string(),
                    offset: 0,
                    field_type: FieldType::DataRange(filesize),
                    description: format!(
                        "Segment data range: 0x{:x} - 0x{:x} ({} bytes)",
                        seg_data_start,
                        seg_data_end - 1,
                        filesize
                    ),
                    editable: false,
                }],
                children: vec![],
            });
        }

        // Parse sections if present
        if nsects > 0 {
            let section_offset = if is_64 { offset + 72 } else { offset + 56 };
            let section_size = if is_64 { 80u64 } else { 68u64 };
            let shown_sections = self.shown_count(nsects as usize);

            for i in 0..shown_sections {
                let sect_off = section_offset + (i as u64 * section_size);
                if let Some(section) = self.parse_section(sect_off, i, is_64) {
                    children.push(section);
                }
            }
        }

        Some(children)
    }

    fn parse_section(&mut self, offset: u64, index: usize, is_64: bool) -> Option<StructDef> {
        let sectname_bytes = read_bytes_raw(self.doc, self.base_offset + offset, 16)?;
        let sectname = String::from_utf8_lossy(&sectname_bytes)
            .trim_end_matches('\0')
            .trim_end_matches(' ')
            .to_string();

        let segname_bytes = read_bytes_raw(self.doc, self.base_offset + offset + 16, 16)?;
        let segname = String::from_utf8_lossy(&segname_bytes)
            .trim_end_matches('\0')
            .trim_end_matches(' ')
            .to_string();

        let (addr, size, file_offset) = if is_64 {
            (
                self.read_u64(offset + 32)?,
                self.read_u64(offset + 40)?,
                self.read_u32(offset + 48)? as u64,
            )
        } else {
            (
                self.read_u32(offset + 32)? as u64,
                self.read_u32(offset + 36)? as u64,
                self.read_u32(offset + 40)? as u64,
            )
        };

        let mut fields = vec![
            FieldDef {
                name: "sectname".to_string(),
                offset: 0,
                field_type: FieldType::Utf8(16),
                description: format!("Section name: {}", sectname),
                editable: true,
            },
            FieldDef {
                name: "segname".to_string(),
                offset: 16,
                field_type: FieldType::Utf8(16),
                description: format!("Segment name: {}", segname),
                editable: true,
            },
            FieldDef {
                name: "addr".to_string(),
                offset: 32,
                field_type: if is_64 { self.u64_t() } else { self.u32_t() },
                description: format!("Virtual address: 0x{:x}", addr),
                editable: true,
            },
            FieldDef {
                name: "size".to_string(),
                offset: if is_64 { 40 } else { 36 },
                field_type: if is_64 { self.u64_t() } else { self.u32_t() },
                description: format!("Size: {} bytes", size),
                editable: false,
            },
        ];

        // Add section data range if there's data
        if size > 0 && file_offset > 0 {
            fields.push(FieldDef {
                name: "section_data".to_string(),
                offset: if is_64 { 48 } else { 40 },
                field_type: FieldType::DataRange(size),
                description: format!(
                    "Section data range: 0x{:x} - 0x{:x}",
                    file_offset,
                    file_offset + size - 1
                ),
                editable: false,
            });
        }

        Some(StructDef {
            name: format!(
                "Section {} [{}]",
                index,
                if sectname.is_empty() {
                    "<unnamed>"
                } else {
                    &sectname
                }
            ),
            base_offset: self.base_offset + offset,
            fields,
            children: vec![],
        })
    }
}

/// Fat Mach-O parser
struct FatParser<'a> {
    doc: &'a mut Document,
    entry_cap: usize,
    is_le: bool,
}

impl<'a> FatParser<'a> {
    fn new(doc: &'a mut Document, is_le: bool, entry_cap: usize) -> Self {
        Self {
            doc,
            entry_cap,
            is_le,
        }
    }

    fn read_u32(&mut self, offset: u64) -> Option<u32> {
        let b = read_bytes_raw(self.doc, offset, 4)?;
        Some(if self.is_le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    }

    fn parse(&mut self) -> Option<FormatDef> {
        let nfat_arch = self.read_u32(4)? as usize;
        if nfat_arch == 0 || nfat_arch > 16 {
            return None;
        }

        let mut children = Vec::with_capacity(nfat_arch);
        let shown = nfat_arch.min(self.entry_cap);

        for i in 0..shown {
            let arch_offset = FAT_HEADER_SIZE + (i as u64 * FAT_ARCH_SIZE);
            if let Some(arch) = self.parse_fat_arch(arch_offset, i) {
                children.push(arch);
            }
        }

        Some(FormatDef {
            name: "Fat Mach-O".to_string(),
            structs: vec![StructDef {
                name: "Fat Header".to_string(),
                base_offset: 0,
                fields: vec![
                    FieldDef {
                        name: "magic".to_string(),
                        offset: 0,
                        field_type: FieldType::U32Be,
                        description: "FAT_MAGIC (Universal Binary)".to_string(),
                        editable: false,
                    },
                    FieldDef {
                        name: "nfat_arch".to_string(),
                        offset: 4,
                        field_type: FieldType::U32Be,
                        description: format!("Number of architectures: {}", nfat_arch),
                        editable: false,
                    },
                ],
                children,
            }],
        })
    }

    fn parse_fat_arch(&mut self, offset: u64, index: usize) -> Option<StructDef> {
        let cputype = self.read_u32(offset)?;
        let cpusubtype = self.read_u32(offset + 4)?;
        let arch_offset = self.read_u32(offset + 8)? as u64;
        let arch_size = self.read_u32(offset + 12)?;
        let align = self.read_u32(offset + 16)?;

        Some(StructDef {
            name: format!("Architecture {} [{}]", index, cpu_type_name(cputype)),
            base_offset: offset,
            fields: vec![
                FieldDef {
                    name: "cputype".to_string(),
                    offset: 0,
                    field_type: FieldType::Enum {
                        inner: Box::new(FieldType::U32Be),
                        variants: vec![
                            (CPU_TYPE_X86 as u64, "x86".to_string()),
                            (CPU_TYPE_X86_64 as u64, "x86_64".to_string()),
                            (CPU_TYPE_ARM as u64, "ARM".to_string()),
                            (CPU_TYPE_ARM64 as u64, "ARM64".to_string()),
                            (CPU_TYPE_POWERPC as u64, "PowerPC".to_string()),
                            (CPU_TYPE_POWERPC64 as u64, "PowerPC64".to_string()),
                        ],
                    },
                    description: format!("CPU type: {}", cpu_type_name(cputype)),
                    editable: false,
                },
                FieldDef {
                    name: "cpusubtype".to_string(),
                    offset: 4,
                    field_type: FieldType::U32Be,
                    description: format!("CPU subtype: 0x{:x}", cpusubtype),
                    editable: true,
                },
                FieldDef {
                    name: "offset".to_string(),
                    offset: 8,
                    field_type: FieldType::U32Be,
                    description: format!("Architecture offset: 0x{:x}", arch_offset),
                    editable: false,
                },
                FieldDef {
                    name: "size".to_string(),
                    offset: 12,
                    field_type: FieldType::U32Be,
                    description: format!("Architecture size: {} bytes", arch_size),
                    editable: false,
                },
                FieldDef {
                    name: "align".to_string(),
                    offset: 16,
                    field_type: FieldType::U32Be,
                    description: format!("Alignment: 2^{} = {} bytes", align, 1u32 << align),
                    editable: false,
                },
            ],
            children: vec![],
        })
    }
}

pub fn detect(doc: &mut Document) -> Option<FormatDef> {
    detect_with_cap(doc, DEFAULT_ENTRY_CAP)
}

pub fn detect_with_cap(doc: &mut Document, entry_cap: usize) -> Option<FormatDef> {
    if doc.len() < 4 {
        return None;
    }

    let magic_bytes = read_bytes_raw(doc, 0, 4)?;
    let magic = u32::from_be_bytes([
        magic_bytes[0],
        magic_bytes[1],
        magic_bytes[2],
        magic_bytes[3],
    ]);

    match magic {
        MH_MAGIC | MH_MAGIC_64 => {
            // Big-endian
            let is_64 = magic == MH_MAGIC_64;
            MachoParser::new(doc, is_64, false, entry_cap).parse()
        }
        MH_CIGAM | MH_CIGAM_64 => {
            // Little-endian (byte-swapped magic)
            let is_64 = magic == MH_CIGAM_64;
            MachoParser::new(doc, is_64, true, entry_cap).parse()
        }
        FAT_MAGIC => {
            // Fat binary, big-endian
            FatParser::new(doc, false, entry_cap).parse()
        }
        FAT_CIGAM => {
            // Fat binary, little-endian
            FatParser::new(doc, true, entry_cap).parse()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_type_name() {
        assert_eq!(cpu_type_name(CPU_TYPE_X86), "x86");
        assert_eq!(cpu_type_name(CPU_TYPE_X86_64), "x86_64");
        assert_eq!(cpu_type_name(CPU_TYPE_ARM64), "ARM64");
        assert_eq!(cpu_type_name(0), "Unknown");
    }

    #[test]
    fn test_file_type_name() {
        assert_eq!(file_type_name(MH_EXECUTE), "Executable");
        assert_eq!(file_type_name(MH_DYLIB), "Dylib");
        assert_eq!(file_type_name(0), "Unknown");
    }

    #[test]
    fn test_load_command_name() {
        assert_eq!(load_command_name(LC_SEGMENT), "LC_SEGMENT");
        assert_eq!(load_command_name(LC_SEGMENT_64), "LC_SEGMENT_64");
        assert_eq!(load_command_name(LC_MAIN), "LC_MAIN");
    }
}
