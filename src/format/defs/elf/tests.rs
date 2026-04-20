use std::fs;

use tempfile::tempdir;

use super::{
    detect_with_cap, DT_NEEDED, DT_NULL, DT_SONAME, ELF64_EHDR_SIZE, ELF64_PHDR_SIZE,
    ELF64_SHDR_SIZE, ELF_MAGIC, GNU_PROPERTY_X86_FEATURE_1_AND, NT_GNU_PROPERTY_TYPE_0, PT_DYNAMIC,
    PT_GNU_PROPERTY, PT_INTERP, PT_LOAD, PT_NOTE, SHT_DYNAMIC, SHT_DYNSYM, SHT_GNU_HASH,
    SHT_GNU_VERDEF, SHT_GNU_VERNEED, SHT_GNU_VERSYM, SHT_HASH, SHT_NOTE, SHT_PROGBITS, SHT_RELA,
    SHT_STRTAB, SHT_SYMTAB,
};
use crate::config::Config;
use crate::core::document::Document;
use crate::format;
use crate::format::parse::StructValue;

const HEADER_SIZE: usize = ELF64_EHDR_SIZE as usize;
const PHDR_OFFSET: usize = HEADER_SIZE;
const PHDR_SIZE: usize = ELF64_PHDR_SIZE as usize;
const TEXT_OFFSET: usize = 0x100;
const SHSTRTAB_OFFSET: usize = 0x120;
const SHDR_OFFSET: usize = 0x200;
const SHDR_SIZE: usize = ELF64_SHDR_SIZE as usize;

fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64_le(buf: &mut [u8], offset: usize, value: u64) {
    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn build_elf64_with_sections(extra_names: &[&str]) -> Vec<u8> {
    let mut strtab = vec![0_u8];
    let mut names = Vec::new();
    for name in [".shstrtab", ".text"]
        .into_iter()
        .chain(extra_names.iter().copied())
    {
        let start = strtab.len() as u32;
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
        names.push((name, start));
    }

    let section_count = 1 + names.len();
    let total_len = SHDR_OFFSET + section_count * SHDR_SIZE;
    let mut bytes = vec![0_u8; total_len.max(SHSTRTAB_OFFSET + strtab.len())];

    bytes[0..4].copy_from_slice(&ELF_MAGIC);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;

    write_u16_le(&mut bytes, 16, 2);
    write_u16_le(&mut bytes, 18, 0x3e);
    write_u32_le(&mut bytes, 20, 1);
    write_u64_le(&mut bytes, 24, 0x401000);
    write_u64_le(&mut bytes, 32, PHDR_OFFSET as u64);
    write_u64_le(&mut bytes, 40, SHDR_OFFSET as u64);
    write_u32_le(&mut bytes, 48, 0);
    write_u16_le(&mut bytes, 52, HEADER_SIZE as u16);
    write_u16_le(&mut bytes, 54, PHDR_SIZE as u16);
    write_u16_le(&mut bytes, 56, 1);
    write_u16_le(&mut bytes, 58, SHDR_SIZE as u16);
    write_u16_le(&mut bytes, 60, section_count as u16);
    write_u16_le(&mut bytes, 62, 1);

    write_u32_le(&mut bytes, PHDR_OFFSET, PT_LOAD);
    write_u32_le(&mut bytes, PHDR_OFFSET + 4, 0x5);
    write_u64_le(&mut bytes, PHDR_OFFSET + 8, TEXT_OFFSET as u64);
    write_u64_le(&mut bytes, PHDR_OFFSET + 16, 0x401000);
    write_u64_le(&mut bytes, PHDR_OFFSET + 24, 0x401000);
    write_u64_le(&mut bytes, PHDR_OFFSET + 32, 4);
    write_u64_le(&mut bytes, PHDR_OFFSET + 40, 4);
    write_u64_le(&mut bytes, PHDR_OFFSET + 48, 0x1000);

    bytes[TEXT_OFFSET..TEXT_OFFSET + 4].copy_from_slice(&[0x90, 0x90, 0x90, 0xc3]);
    bytes[SHSTRTAB_OFFSET..SHSTRTAB_OFFSET + strtab.len()].copy_from_slice(&strtab);

    let shstrtab_name = names[0].1;
    let text_name = names[1].1;

    write_u32_le(&mut bytes, SHDR_OFFSET + SHDR_SIZE, shstrtab_name);
    write_u32_le(&mut bytes, SHDR_OFFSET + SHDR_SIZE + 4, SHT_STRTAB);
    write_u64_le(
        &mut bytes,
        SHDR_OFFSET + SHDR_SIZE + 24,
        SHSTRTAB_OFFSET as u64,
    );
    write_u64_le(
        &mut bytes,
        SHDR_OFFSET + SHDR_SIZE + 32,
        strtab.len() as u64,
    );
    write_u64_le(&mut bytes, SHDR_OFFSET + SHDR_SIZE + 48, 1);

    let text_header = SHDR_OFFSET + SHDR_SIZE * 2;
    write_u32_le(&mut bytes, text_header, text_name);
    write_u32_le(&mut bytes, text_header + 4, SHT_PROGBITS);
    write_u64_le(&mut bytes, text_header + 8, 0x6);
    write_u64_le(&mut bytes, text_header + 16, 0x401000);
    write_u64_le(&mut bytes, text_header + 24, TEXT_OFFSET as u64);
    write_u64_le(&mut bytes, text_header + 32, 4);
    write_u64_le(&mut bytes, text_header + 48, 16);

    for (slot, (_, name_offset)) in names.iter().skip(2).enumerate() {
        let header = SHDR_OFFSET + SHDR_SIZE * (slot + 3);
        write_u32_le(&mut bytes, header, *name_offset);
        write_u32_le(&mut bytes, header + 4, SHT_PROGBITS);
        write_u64_le(&mut bytes, header + 8, 0x2);
        write_u64_le(&mut bytes, header + 48, 1);
    }

    bytes
}

fn build_elf64_with_dynamic_and_notes() -> Vec<u8> {
    const INTERP_OFFSET: usize = 0x180;
    const DYNSTR_OFFSET: usize = 0x1b0;
    const DYNAMIC_OFFSET: usize = 0x1d0;
    const NOTE_OFFSET: usize = 0x200;
    const SHSTRTAB_OFFSET: usize = 0x240;
    const SHDR_OFFSET: usize = 0x2c0;

    let interp = b"/lib64/ld-linux-x86-64.so.2\0";
    let dynstr = b"\0libc.so.6\0sample\0";
    let dynamic_size = 48;

    let note_bytes = {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&NT_GNU_PROPERTY_TYPE_0.to_le_bytes());
        bytes.extend_from_slice(b"GNU\0");
        bytes.extend_from_slice(&GNU_PROPERTY_X86_FEATURE_1_AND.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&[0_u8; 4]);
        bytes
    };

    let mut shstrtab = vec![0_u8];
    let mut name_offsets = Vec::new();
    for name in [
        ".shstrtab",
        ".interp",
        ".dynstr",
        ".dynamic",
        ".note.gnu.property",
    ] {
        let start = shstrtab.len() as u32;
        shstrtab.extend_from_slice(name.as_bytes());
        shstrtab.push(0);
        name_offsets.push(start);
    }

    let section_count = 6;
    let total_len = SHDR_OFFSET + section_count * SHDR_SIZE;
    let mut bytes = vec![0_u8; total_len.max(SHSTRTAB_OFFSET + shstrtab.len())];

    bytes[0..4].copy_from_slice(&ELF_MAGIC);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;

    write_u16_le(&mut bytes, 16, 3);
    write_u16_le(&mut bytes, 18, 0x3e);
    write_u32_le(&mut bytes, 20, 1);
    write_u64_le(&mut bytes, 32, PHDR_OFFSET as u64);
    write_u64_le(&mut bytes, 40, SHDR_OFFSET as u64);
    write_u16_le(&mut bytes, 52, HEADER_SIZE as u16);
    write_u16_le(&mut bytes, 54, PHDR_SIZE as u16);
    write_u16_le(&mut bytes, 56, 4);
    write_u16_le(&mut bytes, 58, SHDR_SIZE as u16);
    write_u16_le(&mut bytes, 60, section_count as u16);
    write_u16_le(&mut bytes, 62, 1);

    bytes[INTERP_OFFSET..INTERP_OFFSET + interp.len()].copy_from_slice(interp);
    bytes[DYNSTR_OFFSET..DYNSTR_OFFSET + dynstr.len()].copy_from_slice(dynstr);
    bytes[NOTE_OFFSET..NOTE_OFFSET + note_bytes.len()].copy_from_slice(&note_bytes);
    bytes[SHSTRTAB_OFFSET..SHSTRTAB_OFFSET + shstrtab.len()].copy_from_slice(&shstrtab);

    // PT_INTERP
    write_u32_le(&mut bytes, PHDR_OFFSET, PT_INTERP);
    write_u32_le(&mut bytes, PHDR_OFFSET + 4, 0x4);
    write_u64_le(&mut bytes, PHDR_OFFSET + 8, INTERP_OFFSET as u64);
    write_u64_le(&mut bytes, PHDR_OFFSET + 32, interp.len() as u64);
    write_u64_le(&mut bytes, PHDR_OFFSET + 40, interp.len() as u64);
    write_u64_le(&mut bytes, PHDR_OFFSET + 48, 1);

    // PT_DYNAMIC
    let dyn_ph = PHDR_OFFSET + PHDR_SIZE;
    write_u32_le(&mut bytes, dyn_ph, PT_DYNAMIC);
    write_u32_le(&mut bytes, dyn_ph + 4, 0x6);
    write_u64_le(&mut bytes, dyn_ph + 8, DYNAMIC_OFFSET as u64);
    write_u64_le(&mut bytes, dyn_ph + 32, dynamic_size as u64);
    write_u64_le(&mut bytes, dyn_ph + 40, dynamic_size as u64);
    write_u64_le(&mut bytes, dyn_ph + 48, 8);

    // PT_NOTE
    let note_ph = PHDR_OFFSET + PHDR_SIZE * 2;
    write_u32_le(&mut bytes, note_ph, PT_NOTE);
    write_u32_le(&mut bytes, note_ph + 4, 0x4);
    write_u64_le(&mut bytes, note_ph + 8, NOTE_OFFSET as u64);
    write_u64_le(&mut bytes, note_ph + 32, note_bytes.len() as u64);
    write_u64_le(&mut bytes, note_ph + 40, note_bytes.len() as u64);
    write_u64_le(&mut bytes, note_ph + 48, 4);

    // PT_GNU_PROPERTY
    let prop_ph = PHDR_OFFSET + PHDR_SIZE * 3;
    write_u32_le(&mut bytes, prop_ph, PT_GNU_PROPERTY);
    write_u32_le(&mut bytes, prop_ph + 4, 0x4);
    write_u64_le(&mut bytes, prop_ph + 8, NOTE_OFFSET as u64);
    write_u64_le(&mut bytes, prop_ph + 32, note_bytes.len() as u64);
    write_u64_le(&mut bytes, prop_ph + 40, note_bytes.len() as u64);
    write_u64_le(&mut bytes, prop_ph + 48, 8);

    // .dynamic entries
    write_u64_le(&mut bytes, DYNAMIC_OFFSET, DT_NEEDED);
    write_u64_le(&mut bytes, DYNAMIC_OFFSET + 8, 1);
    write_u64_le(&mut bytes, DYNAMIC_OFFSET + 16, DT_SONAME);
    write_u64_le(&mut bytes, DYNAMIC_OFFSET + 24, 11);
    write_u64_le(&mut bytes, DYNAMIC_OFFSET + 32, DT_NULL);
    write_u64_le(&mut bytes, DYNAMIC_OFFSET + 40, 0);

    // Section headers
    let shstrtab_sh = SHDR_OFFSET + SHDR_SIZE;
    write_u32_le(&mut bytes, shstrtab_sh, name_offsets[0]);
    write_u32_le(&mut bytes, shstrtab_sh + 4, SHT_STRTAB);
    write_u64_le(&mut bytes, shstrtab_sh + 24, SHSTRTAB_OFFSET as u64);
    write_u64_le(&mut bytes, shstrtab_sh + 32, shstrtab.len() as u64);
    write_u64_le(&mut bytes, shstrtab_sh + 48, 1);

    let interp_sh = SHDR_OFFSET + SHDR_SIZE * 2;
    write_u32_le(&mut bytes, interp_sh, name_offsets[1]);
    write_u32_le(&mut bytes, interp_sh + 4, SHT_PROGBITS);
    write_u64_le(&mut bytes, interp_sh + 8, 0x2);
    write_u64_le(&mut bytes, interp_sh + 24, INTERP_OFFSET as u64);
    write_u64_le(&mut bytes, interp_sh + 32, interp.len() as u64);
    write_u64_le(&mut bytes, interp_sh + 48, 1);

    let dynstr_sh = SHDR_OFFSET + SHDR_SIZE * 3;
    write_u32_le(&mut bytes, dynstr_sh, name_offsets[2]);
    write_u32_le(&mut bytes, dynstr_sh + 4, SHT_STRTAB);
    write_u64_le(&mut bytes, dynstr_sh + 8, 0x2);
    write_u64_le(&mut bytes, dynstr_sh + 24, DYNSTR_OFFSET as u64);
    write_u64_le(&mut bytes, dynstr_sh + 32, dynstr.len() as u64);
    write_u64_le(&mut bytes, dynstr_sh + 48, 1);

    let dynamic_sh = SHDR_OFFSET + SHDR_SIZE * 4;
    write_u32_le(&mut bytes, dynamic_sh, name_offsets[3]);
    write_u32_le(&mut bytes, dynamic_sh + 4, SHT_DYNAMIC);
    write_u64_le(&mut bytes, dynamic_sh + 8, 0x3);
    write_u64_le(&mut bytes, dynamic_sh + 24, DYNAMIC_OFFSET as u64);
    write_u64_le(&mut bytes, dynamic_sh + 32, dynamic_size as u64);
    write_u32_le(&mut bytes, dynamic_sh + 40, 3);
    write_u64_le(&mut bytes, dynamic_sh + 48, 8);
    write_u64_le(&mut bytes, dynamic_sh + 56, 16);

    let note_sh = SHDR_OFFSET + SHDR_SIZE * 5;
    write_u32_le(&mut bytes, note_sh, name_offsets[4]);
    write_u32_le(&mut bytes, note_sh + 4, SHT_NOTE);
    write_u64_le(&mut bytes, note_sh + 8, 0x2);
    write_u64_le(&mut bytes, note_sh + 24, NOTE_OFFSET as u64);
    write_u64_le(&mut bytes, note_sh + 32, note_bytes.len() as u64);
    write_u64_le(&mut bytes, note_sh + 48, 8);

    bytes
}
fn build_elf64_with_symbols_and_versions() -> Vec<u8> {
    #[derive(Clone)]
    struct TestSection {
        name: &'static str,
        sh_type: u32,
        flags: u64,
        data: Vec<u8>,
        link: u32,
        info: u32,
        addralign: u64,
        entsize: u64,
    }

    fn build_strtab(entries: &[&str]) -> (Vec<u8>, Vec<u32>) {
        let mut bytes = vec![0_u8];
        let mut offsets = Vec::with_capacity(entries.len());
        for entry in entries {
            offsets.push(bytes.len() as u32);
            bytes.extend_from_slice(entry.as_bytes());
            bytes.push(0);
        }
        (bytes, offsets)
    }

    fn push_sym64(
        buf: &mut Vec<u8>,
        st_name: u32,
        st_info: u8,
        st_other: u8,
        st_shndx: u16,
        st_value: u64,
        st_size: u64,
    ) {
        buf.extend_from_slice(&st_name.to_le_bytes());
        buf.push(st_info);
        buf.push(st_other);
        buf.extend_from_slice(&st_shndx.to_le_bytes());
        buf.extend_from_slice(&st_value.to_le_bytes());
        buf.extend_from_slice(&st_size.to_le_bytes());
    }

    fn push_rela64(buf: &mut Vec<u8>, r_offset: u64, sym_index: u32, reloc_type: u32) {
        let r_info = ((sym_index as u64) << 32) | reloc_type as u64;
        buf.extend_from_slice(&r_offset.to_le_bytes());
        buf.extend_from_slice(&r_info.to_le_bytes());
        buf.extend_from_slice(&0_i64.to_le_bytes());
    }

    let (strtab, strtab_offsets) = build_strtab(&["local_func"]);
    let local_func_name = strtab_offsets[0];

    let (dynstr, dynstr_offsets) = build_strtab(&["libc.so.6", "puts", "GLIBC_2.2.5", "sample"]);
    let libc_name = dynstr_offsets[0];
    let puts_name = dynstr_offsets[1];
    let glibc_ver = dynstr_offsets[2];
    let sample_name = dynstr_offsets[3];

    let mut symtab = Vec::new();
    push_sym64(&mut symtab, 0, 0, 0, 0, 0, 0);
    push_sym64(&mut symtab, local_func_name, 0x02, 0x00, 0, 0x401000, 4);

    let mut dynsym = Vec::new();
    push_sym64(&mut dynsym, 0, 0, 0, 0, 0, 0);
    push_sym64(&mut dynsym, puts_name, 0x12, 0x00, 0, 0, 0);

    let mut rela = Vec::new();
    push_rela64(&mut rela, 0x404000, 1, 7);

    let mut sysv_hash = Vec::new();
    sysv_hash.extend_from_slice(&1_u32.to_le_bytes());
    sysv_hash.extend_from_slice(&2_u32.to_le_bytes());
    sysv_hash.extend_from_slice(&1_u32.to_le_bytes());
    sysv_hash.extend_from_slice(&0_u32.to_le_bytes());
    sysv_hash.extend_from_slice(&0_u32.to_le_bytes());

    let mut gnu_hash = Vec::new();
    gnu_hash.extend_from_slice(&1_u32.to_le_bytes());
    gnu_hash.extend_from_slice(&1_u32.to_le_bytes());
    gnu_hash.extend_from_slice(&1_u32.to_le_bytes());
    gnu_hash.extend_from_slice(&6_u32.to_le_bytes());
    gnu_hash.extend_from_slice(&1_u64.to_le_bytes());
    gnu_hash.extend_from_slice(&1_u32.to_le_bytes());
    gnu_hash.extend_from_slice(&0x1234_5678_u32.to_le_bytes());

    let mut verdef = Vec::new();
    verdef.extend_from_slice(&1_u16.to_le_bytes());
    verdef.extend_from_slice(&0_u16.to_le_bytes());
    verdef.extend_from_slice(&1_u16.to_le_bytes());
    verdef.extend_from_slice(&1_u16.to_le_bytes());
    verdef.extend_from_slice(&0x1111_u32.to_le_bytes());
    verdef.extend_from_slice(&20_u32.to_le_bytes());
    verdef.extend_from_slice(&0_u32.to_le_bytes());
    verdef.extend_from_slice(&sample_name.to_le_bytes());
    verdef.extend_from_slice(&0_u32.to_le_bytes());

    let mut verneed = Vec::new();
    verneed.extend_from_slice(&1_u16.to_le_bytes());
    verneed.extend_from_slice(&1_u16.to_le_bytes());
    verneed.extend_from_slice(&libc_name.to_le_bytes());
    verneed.extend_from_slice(&16_u32.to_le_bytes());
    verneed.extend_from_slice(&0_u32.to_le_bytes());
    verneed.extend_from_slice(&0x2222_u32.to_le_bytes());
    verneed.extend_from_slice(&0_u16.to_le_bytes());
    verneed.extend_from_slice(&2_u16.to_le_bytes());
    verneed.extend_from_slice(&glibc_ver.to_le_bytes());
    verneed.extend_from_slice(&0_u32.to_le_bytes());

    let mut versym = Vec::new();
    versym.extend_from_slice(&1_u16.to_le_bytes());
    versym.extend_from_slice(&2_u16.to_le_bytes());

    let mut shstrtab = vec![0_u8];
    let mut sh_name_offsets = Vec::new();
    for name in [
        ".shstrtab",
        ".strtab",
        ".symtab",
        ".dynstr",
        ".dynsym",
        ".rela.plt",
        ".hash",
        ".gnu.hash",
        ".gnu.version_d",
        ".gnu.version_r",
        ".gnu.version",
    ] {
        sh_name_offsets.push(shstrtab.len() as u32);
        shstrtab.extend_from_slice(name.as_bytes());
        shstrtab.push(0);
    }

    let sections = vec![
        TestSection {
            name: ".shstrtab",
            sh_type: SHT_STRTAB,
            flags: 0,
            data: shstrtab.clone(),
            link: 0,
            info: 0,
            addralign: 1,
            entsize: 0,
        },
        TestSection {
            name: ".strtab",
            sh_type: SHT_STRTAB,
            flags: 0,
            data: strtab,
            link: 0,
            info: 0,
            addralign: 1,
            entsize: 0,
        },
        TestSection {
            name: ".symtab",
            sh_type: SHT_SYMTAB,
            flags: 0,
            data: symtab,
            link: 2,
            info: 1,
            addralign: 8,
            entsize: 24,
        },
        TestSection {
            name: ".dynstr",
            sh_type: SHT_STRTAB,
            flags: 0x2,
            data: dynstr,
            link: 0,
            info: 0,
            addralign: 1,
            entsize: 0,
        },
        TestSection {
            name: ".dynsym",
            sh_type: SHT_DYNSYM,
            flags: 0x2,
            data: dynsym,
            link: 4,
            info: 1,
            addralign: 8,
            entsize: 24,
        },
        TestSection {
            name: ".rela.plt",
            sh_type: SHT_RELA,
            flags: 0x2,
            data: rela,
            link: 5,
            info: 0,
            addralign: 8,
            entsize: 24,
        },
        TestSection {
            name: ".hash",
            sh_type: SHT_HASH,
            flags: 0x2,
            data: sysv_hash,
            link: 5,
            info: 0,
            addralign: 4,
            entsize: 4,
        },
        TestSection {
            name: ".gnu.hash",
            sh_type: SHT_GNU_HASH,
            flags: 0x2,
            data: gnu_hash,
            link: 5,
            info: 0,
            addralign: 8,
            entsize: 0,
        },
        TestSection {
            name: ".gnu.version_d",
            sh_type: SHT_GNU_VERDEF,
            flags: 0x2,
            data: verdef,
            link: 4,
            info: 0,
            addralign: 4,
            entsize: 0,
        },
        TestSection {
            name: ".gnu.version_r",
            sh_type: SHT_GNU_VERNEED,
            flags: 0x2,
            data: verneed,
            link: 4,
            info: 0,
            addralign: 4,
            entsize: 0,
        },
        TestSection {
            name: ".gnu.version",
            sh_type: SHT_GNU_VERSYM,
            flags: 0x2,
            data: versym,
            link: 5,
            info: 0,
            addralign: 2,
            entsize: 2,
        },
    ];

    let mut offset = 0x100usize;
    let mut placements = Vec::new();
    for section in &sections {
        let align = section.addralign.max(1) as usize;
        offset = offset.div_ceil(align) * align;
        placements.push(offset);
        offset += section.data.len();
    }

    let shoff = offset.div_ceil(0x40) * 0x40;
    let total_len = shoff + (sections.len() + 1) * SHDR_SIZE;
    let mut bytes = vec![0_u8; total_len];

    bytes[0..4].copy_from_slice(&ELF_MAGIC);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    write_u16_le(&mut bytes, 16, 3);
    write_u16_le(&mut bytes, 18, 0x3e);
    write_u32_le(&mut bytes, 20, 1);
    write_u64_le(&mut bytes, 40, shoff as u64);
    write_u16_le(&mut bytes, 52, HEADER_SIZE as u16);
    write_u16_le(&mut bytes, 54, 0);
    write_u16_le(&mut bytes, 56, 0);
    write_u16_le(&mut bytes, 58, SHDR_SIZE as u16);
    write_u16_le(&mut bytes, 60, (sections.len() + 1) as u16);
    write_u16_le(&mut bytes, 62, 1);

    for (section, &section_offset) in sections.iter().zip(&placements) {
        bytes[section_offset..section_offset + section.data.len()].copy_from_slice(&section.data);
    }

    for (index, section) in sections.iter().enumerate() {
        let sh = shoff + SHDR_SIZE * (index + 1);
        let sh_name = sh_name_offsets
            .iter()
            .zip(sections.iter().map(|section| section.name))
            .find_map(|(offset, name)| (name == section.name).then_some(*offset))
            .unwrap();
        write_u32_le(&mut bytes, sh, sh_name);
        write_u32_le(&mut bytes, sh + 4, section.sh_type);
        write_u64_le(&mut bytes, sh + 8, section.flags);
        write_u64_le(&mut bytes, sh + 24, placements[index] as u64);
        write_u64_le(&mut bytes, sh + 32, section.data.len() as u64);
        write_u32_le(&mut bytes, sh + 40, section.link);
        write_u32_le(&mut bytes, sh + 44, section.info);
        write_u64_le(&mut bytes, sh + 48, section.addralign);
        write_u64_le(&mut bytes, sh + 56, section.entsize);
    }

    bytes
}

fn write_elf(path: &std::path::Path, bytes: &[u8]) -> Document {
    fs::write(path, bytes).unwrap();
    Document::open(path, &Config::default()).unwrap()
}

fn find_struct<'a>(structs: &'a [StructValue], needle: &str) -> Option<&'a StructValue> {
    for sv in structs {
        if sv.name.contains(needle) {
            return Some(sv);
        }
        if let Some(found) = find_struct(&sv.children, needle) {
            return Some(found);
        }
    }
    None
}

#[test]
fn detects_section_headers_with_names_and_pagination() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sections.elf");
    let extras: Vec<String> = (0..70).map(|idx| format!(".extra_{idx}")).collect();
    let extra_refs: Vec<&str> = extras.iter().map(String::as_str).collect();
    let mut doc = write_elf(&path, &build_elf64_with_sections(&extra_refs));

    let def = detect_with_cap(&mut doc, 3).expect("ELF should be detected");
    let root = &def.structs[0];
    let table = root
        .children
        .iter()
        .find(|child| child.name.starts_with("Section Header Table"))
        .expect("section table");

    let names: Vec<&str> = table
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect();
    assert!(names.iter().any(|name| name.contains(".shstrtab")));
    assert!(names.iter().any(|name| name.contains(".text")));
    assert!(table
        .children
        .last()
        .unwrap()
        .name
        .contains("use `:insp more` to load more"));
}

#[test]
fn section_data_ranges_point_to_the_actual_section_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("text.elf");
    let mut doc = write_elf(&path, &build_elf64_with_sections(&[]));

    let def = format::detect::detect_format_with_cap(&mut doc, 8).expect("ELF detected");
    let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");
    let data = find_struct(&structs, "Section Data 2").expect("section data child");
    let field = data
        .fields
        .iter()
        .find(|field| field.def.name == "section_data")
        .expect("section_data field");

    assert_eq!(field.abs_offset, TEXT_OFFSET as u64);
    assert_eq!(field.size, 4);
}

#[test]
fn parses_interpreter_and_dynamic_entries() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("dynamic.elf");
    let mut doc = write_elf(&path, &build_elf64_with_dynamic_and_notes());

    let def = format::detect::detect_format_with_cap(&mut doc, 16).expect("ELF detected");
    let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");

    let interpreter = find_struct(&structs, "Interpreter").expect("interp");
    assert_eq!(interpreter.fields[0].def.name, "path");
    assert!(interpreter.fields[0]
        .display
        .contains("ld-linux-x86-64.so.2"));

    let needed = find_struct(&structs, "DT_NEEDED -> libc.so.6").expect("needed entry");
    assert_eq!(needed.fields[0].def.name, "d_tag");

    let soname = find_struct(&structs, "DT_SONAME -> sample").expect("soname entry");
    assert!(soname.name.contains("sample"));
}

#[test]
fn parses_gnu_property_notes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("note.elf");
    let mut doc = write_elf(&path, &build_elf64_with_dynamic_and_notes());

    let def = format::detect::detect_format_with_cap(&mut doc, 16).expect("ELF detected");
    let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");

    let note = find_struct(&structs, "NT_GNU_PROPERTY_TYPE_0").expect("gnu property note");
    assert!(note.name.contains("GNU"));

    let property = find_struct(&structs, "GNU_PROPERTY_X86_FEATURE_1_AND").expect("gnu property");
    assert!(property.name.contains("Property 0"));
}

#[test]
fn parses_symbols_strings_and_relocations() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("symbols.elf");
    let mut doc = write_elf(&path, &build_elf64_with_symbols_and_versions());

    let def = format::detect::detect_format_with_cap(&mut doc, 32).expect("ELF detected");
    let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");

    assert!(find_struct(&structs, "String 0x1: local_func").is_some());
    assert!(find_struct(&structs, "Symbol 1: local_func [FUNC/LOCAL/DEFAULT]").is_some());
    assert!(find_struct(&structs, "Symbol 1: puts [FUNC/GLOBAL/DEFAULT]").is_some());
    assert!(find_struct(&structs, "Relocation 0: R_X86_64_JUMP_SLOT -> puts").is_some());
}

#[test]
fn parses_hash_and_version_sections() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("versions.elf");
    let mut doc = write_elf(&path, &build_elf64_with_symbols_and_versions());

    let def = format::detect::detect_format_with_cap(&mut doc, 32).expect("ELF detected");
    let structs = format::parse::parse_format(&def, &mut doc).expect("parse succeeds");

    assert!(find_struct(&structs, "SysV Hash").is_some());
    assert!(find_struct(&structs, "GNU Hash").is_some());
    assert!(find_struct(&structs, "Verneed 0: libc.so.6").is_some());
    assert!(find_struct(&structs, "Verdaux: sample").is_some());
    assert!(find_struct(&structs, "Versym 1: puts -> GLIBC_2.2.5").is_some());
}
