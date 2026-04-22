use object::read::{ReadCache, ReadCacheOps, ReadRef};
use object::{Object, ObjectSymbol, SymbolKind};

use crate::core::document::Document;
use crate::executable::types::{ExecutableInfo, ImportInfo, SymbolInfo, SymbolSource};

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
                true,
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
    insert_named_symbol(info, symbol.address(), name, source, false);
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
    prefer_existing: bool,
) {
    let raw_name = raw_name.trim();
    if raw_name.is_empty() {
        return;
    }
    let display_name = demangle_symbol(raw_name);
    info.symbols_by_name
        .entry(display_name.clone())
        .or_insert(address);
    if prefer_existing && info.symbols_by_va.contains_key(&address) {
        return;
    }
    info.symbols_by_va.entry(address).or_insert(SymbolInfo {
        display_name,
        raw_name: Some(raw_name.to_owned()),
        source,
    });
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
