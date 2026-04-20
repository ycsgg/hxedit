use super::*;

pub(super) fn data_range_struct(
    name: String,
    base_offset: u64,
    field_name: &str,
    len: u64,
    description: &str,
) -> StructDef {
    StructDef {
        name,
        base_offset,
        fields: vec![FieldDef {
            name: field_name.to_string(),
            offset: 0,
            field_type: FieldType::DataRange(len),
            description: description.to_string(),
            editable: false,
        }],
        children: vec![],
    }
}

pub(super) fn utf8_struct(
    name: String,
    base_offset: u64,
    field_name: &str,
    len: usize,
    description: &str,
) -> StructDef {
    StructDef {
        name,
        base_offset,
        fields: vec![FieldDef {
            name: field_name.to_string(),
            offset: 0,
            field_type: FieldType::Utf8(len),
            description: description.to_string(),
            editable: false,
        }],
        children: vec![],
    }
}

pub(super) fn more_marker(name: String, base_offset: u64) -> StructDef {
    StructDef {
        name,
        base_offset,
        fields: vec![],
        children: vec![],
    }
}

pub(super) fn section_display_name(section: &SectionHeaderInfo) -> &str {
    if section.name.is_empty() {
        match section.sh_type {
            SHT_NULL => "<null>",
            _ => "<unnamed>",
        }
    } else {
        &section.name
    }
}

pub(super) fn align_up(value: u64, align: u64) -> u64 {
    if align <= 1 {
        value
    } else {
        value.saturating_add(align - 1) / align * align
    }
}

pub(super) fn section_link_target(
    sections: &[SectionHeaderInfo],
    link: u32,
) -> Option<&SectionHeaderInfo> {
    sections.get(link as usize)
}

pub(super) fn find_named_section<'a>(
    sections: &'a [SectionHeaderInfo],
    name: &str,
) -> Option<&'a SectionHeaderInfo> {
    sections.iter().find(|section| section.name == name)
}

pub(super) fn program_type_variants() -> Vec<(u64, String)> {
    vec![
        (PT_NULL as u64, "PT_NULL".into()),
        (PT_LOAD as u64, "PT_LOAD".into()),
        (PT_DYNAMIC as u64, "PT_DYNAMIC".into()),
        (PT_INTERP as u64, "PT_INTERP".into()),
        (PT_NOTE as u64, "PT_NOTE".into()),
        (PT_SHLIB as u64, "PT_SHLIB".into()),
        (PT_PHDR as u64, "PT_PHDR".into()),
        (PT_TLS as u64, "PT_TLS".into()),
        (PT_GNU_EH_FRAME as u64, "PT_GNU_EH_FRAME".into()),
        (PT_GNU_STACK as u64, "PT_GNU_STACK".into()),
        (PT_GNU_RELRO as u64, "PT_GNU_RELRO".into()),
        (PT_GNU_PROPERTY as u64, "PT_GNU_PROPERTY".into()),
    ]
}

pub(super) fn section_type_variants() -> Vec<(u64, String)> {
    vec![
        (SHT_NULL as u64, "SHT_NULL".into()),
        (SHT_PROGBITS as u64, "SHT_PROGBITS".into()),
        (SHT_SYMTAB as u64, "SHT_SYMTAB".into()),
        (SHT_STRTAB as u64, "SHT_STRTAB".into()),
        (SHT_RELA as u64, "SHT_RELA".into()),
        (SHT_HASH as u64, "SHT_HASH".into()),
        (SHT_DYNAMIC as u64, "SHT_DYNAMIC".into()),
        (SHT_NOTE as u64, "SHT_NOTE".into()),
        (SHT_NOBITS as u64, "SHT_NOBITS".into()),
        (SHT_REL as u64, "SHT_REL".into()),
        (SHT_DYNSYM as u64, "SHT_DYNSYM".into()),
        (SHT_GNU_HASH as u64, "SHT_GNU_HASH".into()),
        (SHT_GNU_VERDEF as u64, "SHT_GNU_VERDEF".into()),
        (SHT_GNU_VERNEED as u64, "SHT_GNU_VERNEED".into()),
        (SHT_GNU_VERSYM as u64, "SHT_GNU_VERSYM".into()),
    ]
}

pub(super) fn section_flag_variants() -> Vec<(u64, String)> {
    vec![
        (0x1, "WRITE".into()),
        (0x2, "ALLOC".into()),
        (0x4, "EXECINSTR".into()),
        (0x10, "MERGE".into()),
        (0x20, "STRINGS".into()),
        (0x40, "INFO_LINK".into()),
        (0x80, "LINK_ORDER".into()),
        (0x100, "OS_NONCONFORMING".into()),
        (0x200, "GROUP".into()),
        (0x400, "TLS".into()),
    ]
}

pub(super) fn dynamic_tag_variants() -> Vec<(u64, String)> {
    vec![
        (DT_NULL, "DT_NULL".into()),
        (DT_NEEDED, "DT_NEEDED".into()),
        (DT_PLTRELSZ, "DT_PLTRELSZ".into()),
        (DT_PLTGOT, "DT_PLTGOT".into()),
        (DT_HASH, "DT_HASH".into()),
        (DT_STRTAB, "DT_STRTAB".into()),
        (DT_SYMTAB, "DT_SYMTAB".into()),
        (DT_RELA, "DT_RELA".into()),
        (DT_RELASZ, "DT_RELASZ".into()),
        (DT_RELAENT, "DT_RELAENT".into()),
        (DT_STRSZ, "DT_STRSZ".into()),
        (DT_SYMENT, "DT_SYMENT".into()),
        (DT_INIT, "DT_INIT".into()),
        (DT_FINI, "DT_FINI".into()),
        (DT_SONAME, "DT_SONAME".into()),
        (DT_RPATH, "DT_RPATH".into()),
        (DT_SYMBOLIC, "DT_SYMBOLIC".into()),
        (DT_REL, "DT_REL".into()),
        (DT_RELSZ, "DT_RELSZ".into()),
        (DT_RELENT, "DT_RELENT".into()),
        (DT_PLTREL, "DT_PLTREL".into()),
        (DT_DEBUG, "DT_DEBUG".into()),
        (DT_TEXTREL, "DT_TEXTREL".into()),
        (DT_JMPREL, "DT_JMPREL".into()),
        (DT_BIND_NOW, "DT_BIND_NOW".into()),
        (DT_INIT_ARRAY, "DT_INIT_ARRAY".into()),
        (DT_FINI_ARRAY, "DT_FINI_ARRAY".into()),
        (DT_INIT_ARRAYSZ, "DT_INIT_ARRAYSZ".into()),
        (DT_FINI_ARRAYSZ, "DT_FINI_ARRAYSZ".into()),
        (DT_RUNPATH, "DT_RUNPATH".into()),
        (DT_FLAGS, "DT_FLAGS".into()),
        (DT_PREINIT_ARRAY, "DT_PREINIT_ARRAY".into()),
        (DT_PREINIT_ARRAYSZ, "DT_PREINIT_ARRAYSZ".into()),
        (DT_SYMTAB_SHNDX, "DT_SYMTAB_SHNDX".into()),
        (DT_GNU_HASH, "DT_GNU_HASH".into()),
        (DT_FLAGS_1, "DT_FLAGS_1".into()),
        (DT_VERDEF, "DT_VERDEF".into()),
        (DT_VERDEFNUM, "DT_VERDEFNUM".into()),
        (DT_VERNEED, "DT_VERNEED".into()),
        (DT_VERNEEDNUM, "DT_VERNEEDNUM".into()),
    ]
}

pub(super) fn dynamic_tag_label(tag: u64) -> &'static str {
    match tag {
        DT_NULL => "DT_NULL",
        DT_NEEDED => "DT_NEEDED",
        DT_PLTRELSZ => "DT_PLTRELSZ",
        DT_PLTGOT => "DT_PLTGOT",
        DT_HASH => "DT_HASH",
        DT_STRTAB => "DT_STRTAB",
        DT_SYMTAB => "DT_SYMTAB",
        DT_RELA => "DT_RELA",
        DT_RELASZ => "DT_RELASZ",
        DT_RELAENT => "DT_RELAENT",
        DT_STRSZ => "DT_STRSZ",
        DT_SYMENT => "DT_SYMENT",
        DT_INIT => "DT_INIT",
        DT_FINI => "DT_FINI",
        DT_SONAME => "DT_SONAME",
        DT_RPATH => "DT_RPATH",
        DT_SYMBOLIC => "DT_SYMBOLIC",
        DT_REL => "DT_REL",
        DT_RELSZ => "DT_RELSZ",
        DT_RELENT => "DT_RELENT",
        DT_PLTREL => "DT_PLTREL",
        DT_DEBUG => "DT_DEBUG",
        DT_TEXTREL => "DT_TEXTREL",
        DT_JMPREL => "DT_JMPREL",
        DT_BIND_NOW => "DT_BIND_NOW",
        DT_INIT_ARRAY => "DT_INIT_ARRAY",
        DT_FINI_ARRAY => "DT_FINI_ARRAY",
        DT_INIT_ARRAYSZ => "DT_INIT_ARRAYSZ",
        DT_FINI_ARRAYSZ => "DT_FINI_ARRAYSZ",
        DT_RUNPATH => "DT_RUNPATH",
        DT_FLAGS => "DT_FLAGS",
        DT_PREINIT_ARRAY => "DT_PREINIT_ARRAY",
        DT_PREINIT_ARRAYSZ => "DT_PREINIT_ARRAYSZ",
        DT_SYMTAB_SHNDX => "DT_SYMTAB_SHNDX",
        DT_GNU_HASH => "DT_GNU_HASH",
        DT_FLAGS_1 => "DT_FLAGS_1",
        DT_VERDEF => "DT_VERDEF",
        DT_VERDEFNUM => "DT_VERDEFNUM",
        DT_VERNEED => "DT_VERNEED",
        DT_VERNEEDNUM => "DT_VERNEEDNUM",
        _ => "UNKNOWN",
    }
}

pub(super) fn dynamic_tag_uses_string(tag: u64) -> bool {
    matches!(tag, DT_NEEDED | DT_SONAME | DT_RPATH | DT_RUNPATH)
}

pub(super) fn program_type_label(p_type: u32) -> &'static str {
    match p_type {
        PT_NULL => "PT_NULL",
        PT_LOAD => "PT_LOAD",
        PT_DYNAMIC => "PT_DYNAMIC",
        PT_INTERP => "PT_INTERP",
        PT_NOTE => "PT_NOTE",
        PT_SHLIB => "PT_SHLIB",
        PT_PHDR => "PT_PHDR",
        PT_TLS => "PT_TLS",
        PT_GNU_EH_FRAME => "PT_GNU_EH_FRAME",
        PT_GNU_STACK => "PT_GNU_STACK",
        PT_GNU_RELRO => "PT_GNU_RELRO",
        PT_GNU_PROPERTY => "PT_GNU_PROPERTY",
        _ => "UNKNOWN",
    }
}

pub(super) fn section_type_label(sh_type: u32) -> &'static str {
    match sh_type {
        SHT_NULL => "SHT_NULL",
        SHT_PROGBITS => "SHT_PROGBITS",
        SHT_SYMTAB => "SHT_SYMTAB",
        SHT_STRTAB => "SHT_STRTAB",
        SHT_RELA => "SHT_RELA",
        SHT_HASH => "SHT_HASH",
        SHT_DYNAMIC => "SHT_DYNAMIC",
        SHT_NOTE => "SHT_NOTE",
        SHT_NOBITS => "SHT_NOBITS",
        SHT_REL => "SHT_REL",
        SHT_DYNSYM => "SHT_DYNSYM",
        SHT_GNU_HASH => "SHT_GNU_HASH",
        SHT_GNU_VERDEF => "SHT_GNU_VERDEF",
        SHT_GNU_VERNEED => "SHT_GNU_VERNEED",
        SHT_GNU_VERSYM => "SHT_GNU_VERSYM",
        _ => "UNKNOWN",
    }
}

pub(super) fn note_type_label(note_name: &str, note_type: u32) -> &'static str {
    match (note_name, note_type) {
        ("GNU", NT_GNU_BUILD_ID) => "NT_GNU_BUILD_ID",
        ("GNU", NT_GNU_PROPERTY_TYPE_0) => "NT_GNU_PROPERTY_TYPE_0",
        _ => "UNKNOWN",
    }
}

pub(super) fn gnu_property_variants() -> Vec<(u64, String)> {
    vec![
        (
            GNU_PROPERTY_STACK_SIZE as u64,
            "GNU_PROPERTY_STACK_SIZE".into(),
        ),
        (
            GNU_PROPERTY_NO_COPY_ON_PROTECTED as u64,
            "GNU_PROPERTY_NO_COPY_ON_PROTECTED".into(),
        ),
        (
            GNU_PROPERTY_X86_FEATURE_1_AND as u64,
            "GNU_PROPERTY_X86_FEATURE_1_AND".into(),
        ),
        (
            GNU_PROPERTY_X86_ISA_1_USED as u64,
            "GNU_PROPERTY_X86_ISA_1_USED".into(),
        ),
        (
            GNU_PROPERTY_X86_ISA_1_NEEDED as u64,
            "GNU_PROPERTY_X86_ISA_1_NEEDED".into(),
        ),
        (
            GNU_PROPERTY_AARCH64_FEATURE_1_AND as u64,
            "GNU_PROPERTY_AARCH64_FEATURE_1_AND".into(),
        ),
    ]
}

pub(super) fn gnu_property_label(prop_type: u32) -> &'static str {
    match prop_type {
        GNU_PROPERTY_STACK_SIZE => "GNU_PROPERTY_STACK_SIZE",
        GNU_PROPERTY_NO_COPY_ON_PROTECTED => "GNU_PROPERTY_NO_COPY_ON_PROTECTED",
        GNU_PROPERTY_X86_FEATURE_1_AND => "GNU_PROPERTY_X86_FEATURE_1_AND",
        GNU_PROPERTY_X86_ISA_1_USED => "GNU_PROPERTY_X86_ISA_1_USED",
        GNU_PROPERTY_X86_ISA_1_NEEDED => "GNU_PROPERTY_X86_ISA_1_NEEDED",
        GNU_PROPERTY_AARCH64_FEATURE_1_AND => "GNU_PROPERTY_AARCH64_FEATURE_1_AND",
        _ => "UNKNOWN",
    }
}
