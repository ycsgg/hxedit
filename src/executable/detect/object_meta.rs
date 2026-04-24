use object::read::{ReadCache, ReadCacheOps, ReadRef};
use object::{
    Object, ObjectSection, ObjectSymbol, ObjectSymbolTable, RelocationFlags, RelocationTarget,
    SymbolKind,
};

use crate::core::document::Document;
use crate::executable::types::{
    ExecutableArch, ExecutableInfo, ExecutableKind, ImportInfo, SymbolInfo, SymbolSource,
    SymbolType,
};

use super::util::demangle_symbol;

pub(super) fn enrich(doc: &mut Document, info: &mut ExecutableInfo) {
    let doc_len = doc.len();
    if doc_len == 0 {
        return;
    }

    let cache = ReadCache::new(DocumentReadCache::new(doc));
    let data = cache.range(0, doc_len);
    let Ok(file) = object::File::parse(data) else {
        return;
    };

    collect_symbols(&file, info);
    collect_imports(&file, info);
    collect_target_names(&file, info);
}

fn collect_symbols<'data, R>(file: &object::File<'data, R>, info: &mut ExecutableInfo)
where
    R: ReadRef<'data>,
{
    for symbol in file.symbols() {
        insert_object_symbol(info, symbol, SymbolSource::Object);
    }
    for symbol in file.dynamic_symbols() {
        insert_object_symbol(info, symbol, SymbolSource::Dynamic);
    }
    if let Ok(exports) = file.exports() {
        for export in exports {
            insert_named_symbol(
                info,
                export.address(),
                &String::from_utf8_lossy(export.name()),
                SymbolSource::Export,
                0, // Exports don't have size info
                SymbolType::Unknown,
            );
        }
    }
}

fn insert_object_symbol<'data, T>(info: &mut ExecutableInfo, symbol: T, source: SymbolSource)
where
    T: ObjectSymbol<'data>,
{
    if !symbol.is_definition() || symbol.address() == 0 {
        return;
    }
    if matches!(
        symbol.kind(),
        SymbolKind::Section | SymbolKind::File | SymbolKind::Label
    ) {
        return;
    }
    let Ok(name) = symbol.name() else {
        return;
    };
    let symbol_type = match symbol.kind() {
        SymbolKind::Text => SymbolType::Function,
        SymbolKind::Data => SymbolType::Object,
        SymbolKind::Section => SymbolType::Section,
        _ => SymbolType::Unknown,
    };
    let size = symbol.size();
    insert_named_symbol(info, symbol.address(), name, source, size, symbol_type);
}

fn collect_imports<'data, R>(file: &object::File<'data, R>, info: &mut ExecutableInfo)
where
    R: ReadRef<'data>,
{
    let Ok(imports) = file.imports() else {
        return;
    };
    for import in imports {
        let name = String::from_utf8_lossy(import.name()).to_string();
        if name.is_empty() {
            continue;
        }
        let library = (!import.library().is_empty())
            .then(|| String::from_utf8_lossy(import.library()).to_string());
        if info
            .imports
            .iter()
            .any(|existing| existing.name == name && existing.library == library)
        {
            continue;
        }
        info.imports.push(ImportInfo { library, name });
    }
    info.imports.sort_by(|lhs, rhs| {
        lhs.library
            .cmp(&rhs.library)
            .then_with(|| lhs.name.cmp(&rhs.name))
    });
}

fn insert_named_symbol(
    info: &mut ExecutableInfo,
    address: u64,
    raw_name: &str,
    source: SymbolSource,
    size: u64,
    symbol_type: SymbolType,
) {
    let raw_name = raw_name.trim();
    if raw_name.is_empty() {
        return;
    }
    let display_name = demangle_symbol(raw_name);
    info.symbols_by_name
        .entry(display_name.clone())
        .or_insert(address);
    info.symbols_by_va.entry(address).or_insert(SymbolInfo {
        display_name,
        raw_name: Some(raw_name.to_owned()),
        source,
        size,
        symbol_type,
    });
}

fn collect_target_names<'data, R>(file: &object::File<'data, R>, info: &mut ExecutableInfo)
where
    R: ReadRef<'data>,
{
    if info.kind == ExecutableKind::Elf {
        collect_elf_plt_target_names(file, info);
    }
}

fn collect_elf_plt_target_names<'data, R>(file: &object::File<'data, R>, info: &mut ExecutableInfo)
where
    R: ReadRef<'data>,
{
    let Some(layout) = elf_plt_layout(file, info.arch) else {
        return;
    };
    let Some(dynamic_symbols) = file.dynamic_symbol_table() else {
        return;
    };

    let mut slot_index = 0_u64;
    for (_, relocation) in file.dynamic_relocations().into_iter().flatten() {
        let RelocationFlags::Elf { r_type } = relocation.flags() else {
            continue;
        };
        if !is_elf_plt_relocation(info.arch, r_type) {
            continue;
        }
        let RelocationTarget::Symbol(symbol_index) = relocation.target() else {
            continue;
        };
        let Ok(symbol) = dynamic_symbols.symbol_by_index(symbol_index) else {
            continue;
        };
        let Ok(raw_name) = symbol.name() else {
            continue;
        };
        let Some(address) = layout.entry_address(slot_index) else {
            break;
        };
        insert_target_name(info, address, raw_name);
        slot_index = slot_index.saturating_add(1);
    }
}

fn insert_target_name(info: &mut ExecutableInfo, address: u64, raw_name: &str) {
    let raw_name = raw_name.trim();
    if raw_name.is_empty() {
        return;
    }
    let display_name = demangle_symbol(raw_name);
    if display_name.is_empty() || info.symbols_by_va.contains_key(&address) {
        return;
    }
    info.target_names_by_va
        .entry(address)
        .or_insert(display_name);
}

fn is_elf_plt_relocation(arch: ExecutableArch, r_type: u32) -> bool {
    matches!(
        (arch, r_type),
        (ExecutableArch::X86, 7) | (ExecutableArch::X86_64, 7) | (ExecutableArch::AArch64, 1026)
    )
}

fn elf_plt_layout<'data, R>(
    file: &object::File<'data, R>,
    arch: ExecutableArch,
) -> Option<ElfPltLayout>
where
    R: ReadRef<'data>,
{
    let (header_size, entry_size) = match arch {
        ExecutableArch::X86 | ExecutableArch::X86_64 => (16, 16),
        ExecutableArch::AArch64 => (32, 16),
        _ => return None,
    };

    if matches!(arch, ExecutableArch::X86 | ExecutableArch::X86_64) {
        if let Some(section) = file.section_by_name(".plt.sec") {
            let layout = ElfPltLayout::new(section.address(), 0, section.size(), entry_size);
            if layout.entry_count > 0 {
                return Some(layout);
            }
        }
    }

    let section = file.section_by_name(".plt")?;
    let layout = ElfPltLayout::new(section.address(), header_size, section.size(), entry_size);
    (layout.entry_count > 0).then_some(layout)
}

struct ElfPltLayout {
    start: u64,
    header_size: u64,
    entry_size: u64,
    entry_count: u64,
}

impl ElfPltLayout {
    fn new(start: u64, header_size: u64, section_size: u64, entry_size: u64) -> Self {
        let body_size = section_size.saturating_sub(header_size);
        Self {
            start,
            header_size,
            entry_size,
            entry_count: body_size / entry_size,
        }
    }

    fn entry_address(&self, slot_index: u64) -> Option<u64> {
        (slot_index < self.entry_count).then_some(
            self.start
                .saturating_add(self.header_size)
                .saturating_add(slot_index.saturating_mul(self.entry_size)),
        )
    }
}

struct DocumentReadCache<'a> {
    doc: &'a mut Document,
    position: u64,
}

impl<'a> DocumentReadCache<'a> {
    fn new(doc: &'a mut Document) -> Self {
        Self { doc, position: 0 }
    }
}

impl ReadCacheOps for DocumentReadCache<'_> {
    fn len(&mut self) -> Result<u64, ()> {
        Ok(self.doc.len())
    }

    fn seek(&mut self, pos: u64) -> Result<u64, ()> {
        if pos > self.doc.len() {
            return Err(());
        }
        self.position = pos;
        Ok(pos)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        if buf.is_empty() {
            return Ok(0);
        }
        let bytes = self
            .doc
            .read_logical_range(self.position, buf.len())
            .map_err(|_| ())?;
        let read = bytes.len();
        buf[..read].copy_from_slice(&bytes);
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ()> {
        if buf.is_empty() {
            return Ok(());
        }
        let bytes = self
            .doc
            .read_logical_range(self.position, buf.len())
            .map_err(|_| ())?;
        if bytes.len() != buf.len() {
            return Err(());
        }
        buf.copy_from_slice(&bytes);
        self.position = self.position.saturating_add(buf.len() as u64);
        Ok(())
    }
}
