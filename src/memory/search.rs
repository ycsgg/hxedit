use crate::error::{HxError, HxResult};
use crate::util::parse::{parse_hex_stream, parse_offset};

use super::{MemoryRegion, RegionKind};

const DEFAULT_MAX_REGION_LEN: u64 = 1024 * 1024 * 1024;
const SEARCH_CHUNK: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearchQuery {
    pub pattern: Vec<u8>,
    pub filter: MemoryRegionFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryRegionFilter {
    clauses: Vec<FilterClause>,
    explicit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FilterClause {
    include: bool,
    selectors: Vec<FilterSelector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FilterSelector {
    Any,
    Permissions([PermissionSlot; 3]),
    Kind(RegionKind),
    Kinds(Vec<RegionKind>),
    PathGlob(String),
    VaRange { start: u64, end: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionSlot {
    Required,
    Absent,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearchHit {
    pub region_index: usize,
    pub addr: u64,
    pub wrapped: bool,
    pub skipped_regions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySearchDirection {
    Forward,
    Backward,
}

impl MemorySearchQuery {
    pub fn parse(input: &str) -> HxResult<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(HxError::MissingArgument("memory search pattern"));
        }
        let (mode, body, rest) = parse_delimited_pattern(trimmed)?;
        let pattern = parse_pattern(mode, body)?;
        if pattern.is_empty() {
            return Err(HxError::EmptySearch);
        }
        let filter = MemoryRegionFilter::parse(rest)?;
        Ok(Self { pattern, filter })
    }
}

impl MemoryRegionFilter {
    pub fn parse(input: &str) -> HxResult<Self> {
        let mut filter = Self::default();
        for clause in input.split_whitespace() {
            let (include, selectors) = if let Some(rest) = clause.strip_prefix("in:") {
                (true, rest)
            } else if let Some(rest) = clause.strip_prefix("not:") {
                (false, rest)
            } else {
                return Err(HxError::UnknownCommand(format!("ms {clause}")));
            };
            if selectors.is_empty() {
                return Err(HxError::MissingArgument("memory search filter selector"));
            }
            let selectors = selectors
                .split(',')
                .map(parse_selector)
                .collect::<HxResult<Vec<_>>>()?;
            filter.explicit = true;
            filter.clauses.push(FilterClause { include, selectors });
        }
        Ok(filter)
    }

    pub fn matches(&self, region: &MemoryRegion) -> bool {
        if !region.permissions.read {
            return false;
        }
        if !self.explicit {
            return !matches!(region.kind, RegionKind::Vsyscall | RegionKind::Vvar)
                && region.len() <= DEFAULT_MAX_REGION_LEN;
        }
        self.clauses.iter().all(|clause| {
            let matched = clause
                .selectors
                .iter()
                .any(|selector| selector_matches(selector, region));
            matched == clause.include
        })
    }

    pub(crate) fn clamp_search_range(
        &self,
        region: &MemoryRegion,
        start: u64,
        end: u64,
    ) -> Option<(u64, u64)> {
        let mut range = (start.max(region.start), end.min(region.end));
        for clause in &self.clauses {
            if !clause.include {
                continue;
            }
            let mut va_bounds = clause
                .selectors
                .iter()
                .filter_map(|selector| match selector {
                    FilterSelector::VaRange { start, end } => Some((*start, *end)),
                    _ => None,
                })
                .peekable();
            if va_bounds.peek().is_none() {
                continue;
            }
            let mut clause_start = u64::MAX;
            let mut clause_end = 0;
            for (start, end) in va_bounds {
                clause_start = clause_start.min(start);
                clause_end = clause_end.max(end);
            }
            range.0 = range.0.max(clause_start);
            range.1 = range.1.min(clause_end);
        }
        (range.0 < range.1).then_some(range)
    }
}

pub(crate) fn search_bytes_forward(bytes: &[u8], pattern: &[u8]) -> Option<usize> {
    let mut matcher = KmpMatcher::new(pattern);
    for (idx, byte) in bytes.iter().enumerate() {
        if matcher.feed(*byte) {
            return Some(idx + 1 - pattern.len());
        }
    }
    None
}

pub(crate) fn search_bytes_backward(bytes: &[u8], pattern: &[u8]) -> Option<usize> {
    let reversed_pattern = pattern.iter().rev().copied().collect::<Vec<_>>();
    let mut matcher = KmpMatcher::new(&reversed_pattern);
    for (idx, byte) in bytes.iter().enumerate().rev() {
        if matcher.feed(*byte) {
            return Some(idx);
        }
    }
    None
}

pub(crate) fn search_region_forward(
    mut read_range: impl FnMut(u64, usize) -> HxResult<Vec<u8>>,
    start: u64,
    end: u64,
    pattern: &[u8],
) -> HxResult<Option<u64>> {
    if pattern.is_empty() || start >= end || end - start < pattern.len() as u64 {
        return Ok(None);
    }
    let mut cursor = start;
    let mut overlap = Vec::new();
    while cursor < end {
        let len = (end - cursor).min(SEARCH_CHUNK) as usize;
        let chunk = read_range(cursor, len)?;
        if chunk.is_empty() {
            break;
        }
        let base = cursor.saturating_sub(overlap.len() as u64);
        let mut searchable = overlap;
        searchable.extend_from_slice(&chunk);
        if let Some(pos) = search_bytes_forward(&searchable, pattern) {
            let addr = base + pos as u64;
            if addr >= start {
                return Ok(Some(addr));
            }
        }
        let keep = pattern.len().saturating_sub(1).min(searchable.len());
        overlap = searchable[searchable.len() - keep..].to_vec();
        cursor += chunk.len() as u64;
    }
    Ok(None)
}

pub(crate) fn search_region_backward(
    mut read_range: impl FnMut(u64, usize) -> HxResult<Vec<u8>>,
    start: u64,
    end: u64,
    pattern: &[u8],
) -> HxResult<Option<u64>> {
    if pattern.is_empty() || start >= end || end - start < pattern.len() as u64 {
        return Ok(None);
    }
    let mut cursor = end;
    let mut overlap = Vec::new();
    while cursor > start {
        let len = (cursor - start).min(SEARCH_CHUNK) as usize;
        let chunk_start = cursor - len as u64;
        let chunk = read_range(chunk_start, len)?;
        if chunk.is_empty() {
            break;
        }
        let mut searchable = chunk;
        searchable.extend_from_slice(&overlap);
        if let Some(pos) = search_bytes_backward(&searchable, pattern) {
            let addr = chunk_start + pos as u64;
            if addr + pattern.len() as u64 <= end {
                return Ok(Some(addr));
            }
        }
        let keep = pattern.len().saturating_sub(1).min(searchable.len());
        overlap = searchable[..keep].to_vec();
        cursor = chunk_start;
    }
    Ok(None)
}

fn parse_delimited_pattern(input: &str) -> HxResult<(&str, &str, &str)> {
    let delimiter_index = input
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_ascii_alphanumeric()).then_some((idx, ch)))
        .ok_or(HxError::MissingArgument("memory search delimiter"))?;
    let (delim_idx, delimiter) = delimiter_index;
    let mode = &input[..delim_idx];
    let body_start = delim_idx + delimiter.len_utf8();
    let body = &input[body_start..];
    let Some(end_rel) = body.find(delimiter) else {
        return Err(HxError::MissingArgument("memory search closing delimiter"));
    };
    let pattern = &body[..end_rel];
    let rest = body[end_rel + delimiter.len_utf8()..].trim();
    Ok((mode, pattern, rest))
}

fn parse_pattern(mode: &str, body: &str) -> HxResult<Vec<u8>> {
    match mode {
        "" | "s" | "str" | "utf8" => Ok(body.as_bytes().to_vec()),
        "x" | "hex" => parse_hex_stream(body),
        "b" | "byte" => {
            let value = parse_offset(body)?;
            let byte = u8::try_from(value).map_err(|_| HxError::InvalidOffset(body.to_owned()))?;
            Ok(vec![byte])
        }
        "u32" | "u32le" => parse_u32(body).map(|value| value.to_le_bytes().to_vec()),
        "u32be" => parse_u32(body).map(|value| value.to_be_bytes().to_vec()),
        "u64" | "u64le" => parse_offset(body).map(|value| value.to_le_bytes().to_vec()),
        "u64be" => parse_offset(body).map(|value| value.to_be_bytes().to_vec()),
        "i32" | "i32le" => parse_i32(body).map(|value| value.to_le_bytes().to_vec()),
        "i32be" => parse_i32(body).map(|value| value.to_be_bytes().to_vec()),
        "i64" | "i64le" => parse_i64(body).map(|value| value.to_le_bytes().to_vec()),
        "i64be" => parse_i64(body).map(|value| value.to_be_bytes().to_vec()),
        other => Err(HxError::UnknownCommand(format!("ms {other}/.../"))),
    }
}

fn parse_u32(input: &str) -> HxResult<u32> {
    u32::try_from(parse_offset(input)?).map_err(|_| HxError::InvalidOffset(input.to_owned()))
}

fn parse_i32(input: &str) -> HxResult<i32> {
    i32::try_from(parse_i64(input)?).map_err(|_| HxError::InvalidOffset(input.to_owned()))
}

fn parse_i64(input: &str) -> HxResult<i64> {
    let trimmed = input.trim();
    if let Some(hex) = trimmed.strip_prefix("-0x") {
        let value =
            i64::from_str_radix(hex, 16).map_err(|_| HxError::InvalidOffset(input.to_owned()))?;
        Ok(-value)
    } else if let Some(hex) = trimmed.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).map_err(|_| HxError::InvalidOffset(input.to_owned()))
    } else {
        trimmed
            .parse::<i64>()
            .map_err(|_| HxError::InvalidOffset(input.to_owned()))
    }
}

fn parse_selector(input: &str) -> HxResult<FilterSelector> {
    if input == "*" {
        return Ok(FilterSelector::Any);
    }
    if let Some(path) = input.strip_prefix("path:") {
        return Ok(FilterSelector::PathGlob(path.to_owned()));
    }
    if let Some(range) = input.strip_prefix("va:") {
        let (start, end) = range
            .split_once('-')
            .ok_or_else(|| HxError::InvalidOffset(range.to_owned()))?;
        let start = parse_offset(start)?;
        let end = parse_offset(end)?;
        if start >= end {
            return Err(HxError::InvalidOffset(range.to_owned()));
        }
        return Ok(FilterSelector::VaRange { start, end });
    }
    if let Some(profile) = input.strip_prefix('@') {
        return parse_profile(profile);
    }
    if let Some(perms) = parse_permissions(input) {
        return Ok(FilterSelector::Permissions(perms));
    }
    parse_kind(input)
        .map(FilterSelector::Kind)
        .ok_or_else(|| HxError::UnknownCommand(format!("ms selector {input}")))
}

fn parse_permissions(input: &str) -> Option<[PermissionSlot; 3]> {
    if input.len() != 3 {
        return None;
    }
    let mut chars = input.chars();
    Some([
        parse_permission_slot(chars.next()?, 'r')?,
        parse_permission_slot(chars.next()?, 'w')?,
        parse_permission_slot(chars.next()?, 'x')?,
    ])
}

fn parse_permission_slot(ch: char, expected: char) -> Option<PermissionSlot> {
    match ch {
        '*' => Some(PermissionSlot::Any),
        '-' => Some(PermissionSlot::Absent),
        value if value == expected => Some(PermissionSlot::Required),
        _ => None,
    }
}

fn parse_kind(input: &str) -> Option<RegionKind> {
    match input {
        "heap" => Some(RegionKind::Heap),
        "stack" => Some(RegionKind::Stack),
        "anon" => Some(RegionKind::Anonymous),
        "mapped" => Some(RegionKind::Mapped),
        "private" => Some(RegionKind::Private),
        "shared" => Some(RegionKind::Shared),
        "module" => Some(RegionKind::Module),
        "vdso" => Some(RegionKind::Vdso),
        "vsyscall" => Some(RegionKind::Vsyscall),
        "vvar" => Some(RegionKind::Vvar),
        _ => None,
    }
}

fn parse_profile(input: &str) -> HxResult<FilterSelector> {
    match input {
        "writable" | "data" => Ok(FilterSelector::Permissions([
            PermissionSlot::Required,
            PermissionSlot::Required,
            PermissionSlot::Absent,
        ])),
        "code" => Ok(FilterSelector::Permissions([
            PermissionSlot::Required,
            PermissionSlot::Absent,
            PermissionSlot::Required,
        ])),
        "heapstack" => Ok(FilterSelector::Kinds(vec![
            RegionKind::Heap,
            RegionKind::Stack,
        ])),
        "modules" => Ok(FilterSelector::Kind(RegionKind::Module)),
        other => Err(HxError::UnknownCommand(format!("ms profile @{other}"))),
    }
}

fn selector_matches(selector: &FilterSelector, region: &MemoryRegion) -> bool {
    match selector {
        FilterSelector::Any => true,
        FilterSelector::Permissions(mask) => permission_matches(mask, region),
        FilterSelector::Kind(kind) => region.kind == *kind,
        FilterSelector::Kinds(kinds) => kinds.contains(&region.kind),
        FilterSelector::PathGlob(glob) => region
            .path
            .as_ref()
            .and_then(|path| path.to_str())
            .is_some_and(|path| glob_matches(glob, path)),
        FilterSelector::VaRange { start, end } => region.start < *end && *start < region.end,
    }
}

#[derive(Debug)]
struct KmpMatcher<'a> {
    pattern: &'a [u8],
    prefix: Vec<usize>,
    matched: usize,
}

impl<'a> KmpMatcher<'a> {
    fn new(pattern: &'a [u8]) -> Self {
        let mut prefix = vec![0; pattern.len()];
        let mut matched = 0;
        for idx in 1..pattern.len() {
            while matched > 0 && pattern[idx] != pattern[matched] {
                matched = prefix[matched - 1];
            }
            if pattern[idx] == pattern[matched] {
                matched += 1;
                prefix[idx] = matched;
            }
        }
        Self {
            pattern,
            prefix,
            matched: 0,
        }
    }

    fn feed(&mut self, byte: u8) -> bool {
        while self.matched > 0 && byte != self.pattern[self.matched] {
            self.matched = self.prefix[self.matched - 1];
        }
        if byte == self.pattern[self.matched] {
            self.matched += 1;
            if self.matched == self.pattern.len() {
                self.matched = self.prefix[self.matched - 1];
                return true;
            }
        }
        false
    }
}

fn permission_matches(mask: &[PermissionSlot; 3], region: &MemoryRegion) -> bool {
    [
        (mask[0], region.permissions.read),
        (mask[1], region.permissions.write),
        (mask[2], region.permissions.execute),
    ]
    .into_iter()
    .all(|(slot, present)| match slot {
        PermissionSlot::Required => present,
        PermissionSlot::Absent => !present,
        PermissionSlot::Any => true,
    })
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let (mut pi, mut vi) = (0, 0);
    let (mut star, mut star_vi) = (None, 0);
    let p = pattern.as_bytes();
    let v = value.as_bytes();
    while vi < v.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            pi += 1;
            star_vi = vi;
        } else if let Some(star_pi) = star {
            pi = star_pi + 1;
            star_vi += 1;
            vi = star_vi;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryPermissions, RegionFingerprint};

    #[test]
    fn query_parses_modes_and_filters() {
        let query = MemorySearchQuery::parse("x/4889c7/ in:r-x not:va:0x1000-0x2000").unwrap();
        assert_eq!(query.pattern, vec![0x48, 0x89, 0xc7]);
        assert_eq!(query.filter.clauses.len(), 2);

        let query = MemorySearchQuery::parse("u32/4660/ in:rw-").unwrap();
        assert_eq!(query.pattern, 4660_u32.to_le_bytes());
    }

    #[test]
    fn query_rejects_narrow_integer_overflow() {
        assert!(matches!(
            MemorySearchQuery::parse("u32/0x100000000/"),
            Err(HxError::InvalidOffset(_))
        ));
        assert!(matches!(
            MemorySearchQuery::parse("i32/2147483648/"),
            Err(HxError::InvalidOffset(_))
        ));
    }

    #[test]
    fn filters_match_permissions_kind_and_path() {
        let mut region = MemoryRegion::new(
            0x1000,
            0x2000,
            MemoryPermissions::read_write(),
            RegionKind::Heap,
            RegionFingerprint(1),
        );
        region.path = Some("/tmp/libdemo.so".into());

        assert!(MemoryRegionFilter::parse("in:rw- in:heap")
            .unwrap()
            .matches(&region));
        assert!(MemoryRegionFilter::parse("in:path:*demo.so")
            .unwrap()
            .matches(&region));
        assert!(!MemoryRegionFilter::parse("not:heap")
            .unwrap()
            .matches(&region));

        let stack = MemoryRegion::new(
            0x3000,
            0x4000,
            MemoryPermissions::readable(),
            RegionKind::Stack,
            RegionFingerprint(2),
        );
        assert!(MemoryRegionFilter::parse("in:@heapstack")
            .unwrap()
            .matches(&region));
        assert!(MemoryRegionFilter::parse("in:@heapstack")
            .unwrap()
            .matches(&stack));
    }

    #[test]
    fn va_filter_clamps_scan_range_and_rejects_reversed_ranges() {
        let region = MemoryRegion::new(
            0x1000,
            0x2000,
            MemoryPermissions::readable(),
            RegionKind::Anonymous,
            RegionFingerprint(1),
        );
        let filter = MemoryRegionFilter::parse("in:va:0x1200-0x1300").unwrap();
        assert_eq!(
            filter.clamp_search_range(&region, 0x1000, 0x2000),
            Some((0x1200, 0x1300))
        );
        assert!(matches!(
            MemoryRegionFilter::parse("in:va:0x1300-0x1200"),
            Err(HxError::InvalidOffset(_))
        ));
    }

    #[test]
    fn chunked_search_finds_boundary_matches() {
        let bytes = [b'a', b'b', b'c', b'd'];
        let found = search_region_forward(
            |addr, len| Ok(bytes[addr as usize..addr as usize + len].to_vec()),
            0,
            bytes.len() as u64,
            b"bc",
        )
        .unwrap();
        assert_eq!(found, Some(1));

        let found = search_region_backward(
            |addr, len| Ok(bytes[addr as usize..addr as usize + len].to_vec()),
            0,
            bytes.len() as u64,
            b"bc",
        )
        .unwrap();
        assert_eq!(found, Some(1));
    }
}
