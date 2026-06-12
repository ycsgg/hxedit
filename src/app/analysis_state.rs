use std::collections::HashMap;
use std::panic;
use std::thread;

use crate::app::symbol_state::{
    SymbolNameKind, SymbolPanelEntry, SymbolPanelEntrySource, SymbolPanelSource,
};
use crate::app::{App, EditOp, SidePanelKind, SymbolState};
use crate::core::document::Document;
use crate::disasm::{DisasmFunctionBoundary, DisasmFunctionScope, DisasmRow, DisasmRowKind};
use crate::error::{HxError, HxResult};
use crate::executable::SymbolType;

const SAGITTA_INPUT_LIMIT: u64 = 128 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum BackgroundJobResult {
    SagittaAnalysis {
        job_id: u64,
        revision: u64,
        result: Result<SagittaSnapshot, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SagittaStatus {
    Running,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnalysisValidity {
    Current,
    OutdatedBytes,
    InvalidLayout,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct SagittaAnalysisState {
    pub status: SagittaStatus,
    pub validity: AnalysisValidity,
    pub revision: u64,
    pub snapshot: Option<SagittaSnapshot>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct SagittaSnapshot {
    pub summary: SagittaSummary,
    pub functions: Vec<RecoveredFunction>,
    pub blocks: Vec<RecoveredBlock>,
    pub cfg_edges: Vec<RecoveredCfgEdge>,
    pub call_edges: Vec<RecoveredCallEdge>,
    pub diagnostics: Vec<RecoveredDiagnostic>,
    function_entry_index: Vec<(u64, usize)>,
    function_scope_index: Vec<FunctionScopeIndexEntry>,
}

#[derive(Debug, Clone)]
struct FunctionScopeIndexEntry {
    start_va: u64,
    end_va: u64,
    function_start: u64,
    function_end: u64,
    function_idx: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
pub(crate) struct SagittaSummary {
    pub functions: usize,
    pub blocks: usize,
    pub cfg_edges: usize,
    pub call_edges: usize,
    pub diagnostics: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RecoveredFunction {
    pub entry_va: u64,
    pub entry_logical_offset: Option<u64>,
    pub name: String,
    pub name_kind: SymbolNameKind,
    pub confidence: RecoveredConfidence,
    pub provenance: Vec<RecoveredFunctionSource>,
    pub blocks: Vec<u64>,
    pub callers: Vec<RecoveredCallRef>,
    pub callees: Vec<RecoveredCallRef>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RecoveredBlock {
    pub start_va: u64,
    pub end_va: u64,
    pub logical_offset: Option<u64>,
    pub size: u32,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RecoveredCfgEdge {
    pub src_va: u64,
    pub dst_va: u64,
    pub kind: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RecoveredCallEdge {
    pub caller_va: u64,
    pub callee_va: u64,
    pub kind: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RecoveredCallRef {
    pub function_va: u64,
    pub kind: String,
    pub sites: Vec<u64>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RecoveredDiagnostic {
    pub va: u64,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveredConfidence {
    Pinned,
    Structural,
    Heuristic,
    AiProposed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveredFunctionSource {
    Entry,
    Symbol,
    DirectCall,
    PltStub,
    ThunkTarget,
    ElfLibcStartMain,
    InitArray,
    FiniArray,
    PeExport,
    PeTlsCallback,
    PeEntryThunk,
    CetPrologue,
    ElfInitSection,
    ElfFiniSection,
    ElfFde,
    PeRuntimeFunction,
    External,
    GoPclntab,
}

impl SagittaSnapshot {
    pub(crate) fn new(
        summary: SagittaSummary,
        functions: Vec<RecoveredFunction>,
        blocks: Vec<RecoveredBlock>,
        cfg_edges: Vec<RecoveredCfgEdge>,
        call_edges: Vec<RecoveredCallEdge>,
        diagnostics: Vec<RecoveredDiagnostic>,
    ) -> Self {
        let function_entry_index = build_function_entry_index(&functions);
        let function_scope_index = build_function_scope_index(&functions, &blocks);
        Self {
            summary,
            functions,
            blocks,
            cfg_edges,
            call_edges,
            diagnostics,
            function_entry_index,
            function_scope_index,
        }
    }

    pub(crate) fn symbol_entries(&self) -> Vec<SymbolPanelEntry> {
        let mut entries = self
            .functions
            .iter()
            .map(|function| SymbolPanelEntry {
                address: function.entry_va,
                name: function.name.clone(),
                name_kind: function.name_kind,
                size: 0,
                symbol_type: SymbolType::Function,
                source: SymbolPanelEntrySource::Sagitta,
                logical_offset: function.entry_logical_offset,
                file_offset: function.entry_logical_offset,
                confidence_label: Some(confidence_label(function.confidence).to_owned()),
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.address);
        entries
    }
}

impl App {
    pub(crate) fn start_sagitta_analysis(&mut self) -> HxResult<()> {
        if self
            .analysis_state
            .as_ref()
            .is_some_and(|state| matches!(state.status, SagittaStatus::Running))
        {
            return Ok(());
        }

        let bytes = materialize_logical_bytes(&mut self.document)?;
        validate_sagitta_supported_input(&bytes)?;
        let revision = self.document_revision;
        let job_id = self.analysis_job_id.saturating_add(1);
        self.analysis_job_id = job_id;
        self.analysis_state = Some(SagittaAnalysisState {
            status: SagittaStatus::Running,
            validity: AnalysisValidity::Current,
            revision,
            snapshot: None,
        });

        let tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = panic::catch_unwind(|| analyze_sagitta_bytes(bytes))
                .unwrap_or_else(|payload| Err(panic_message(payload)));
            let _ = tx.send(BackgroundJobResult::SagittaAnalysis {
                job_id,
                revision,
                result,
            });
        });
        self.set_info_status("Sagitta analysis running");
        Ok(())
    }

    pub(crate) fn clear_sagitta_analysis(&mut self) {
        self.analysis_job_id = self.analysis_job_id.saturating_add(1);
        self.analysis_state = None;
        if self
            .symbol_state
            .as_ref()
            .is_some_and(|state| state.source == SymbolPanelSource::Sagitta)
        {
            self.symbol_state = None;
            self.restore_inspector_after_side_panel_close();
        }
        self.set_info_status("Sagitta analysis off");
    }

    pub(crate) fn sagitta_analysis_status_message(&self) -> String {
        let Some(state) = &self.analysis_state else {
            return "analysis idle".to_owned();
        };
        match &state.status {
            SagittaStatus::Running => "analysis running".to_owned(),
            SagittaStatus::Failed(message) => format!("analysis failed: {message}"),
            SagittaStatus::Ready => {
                let functions = state
                    .snapshot
                    .as_ref()
                    .map_or(0, |snapshot| snapshot.summary.functions);
                match state.validity {
                    AnalysisValidity::Current => {
                        format!("analysis ready; {functions} functions")
                    }
                    AnalysisValidity::OutdatedBytes => {
                        format!("analysis outdated; rerun :ana; {functions} functions")
                    }
                    AnalysisValidity::InvalidLayout => {
                        format!("analysis offsets changed; rerun :ana; {functions} functions")
                    }
                }
            }
        }
    }

    pub(crate) fn drain_background_results(&mut self) {
        while let Ok(result) = self.background_rx.try_recv() {
            match result {
                BackgroundJobResult::SagittaAnalysis {
                    job_id,
                    revision,
                    result,
                } => self.handle_sagitta_result(job_id, revision, result),
            }
        }
    }

    pub(crate) fn open_sagitta_symbol_panel_if_ready(&mut self) -> bool {
        let Some(entries) = self
            .ready_sagitta_snapshot()
            .map(SagittaSnapshot::symbol_entries)
        else {
            return false;
        };
        let functions = entries.len();
        self.symbol_state = Some(SymbolState::from_entries(
            entries,
            SymbolPanelSource::Sagitta,
        ));
        self.show_side_panel = true;
        self.focus_symbol_panel();
        self.set_info_status(format!("symbol view (Sagitta, {functions} functions)"));
        true
    }

    pub(crate) fn sagitta_symbol_offsets_invalid(&self) -> bool {
        self.symbol_state
            .as_ref()
            .is_some_and(|state| state.source == SymbolPanelSource::Sagitta)
            && self
                .analysis_state
                .as_ref()
                .is_some_and(|state| state.validity == AnalysisValidity::InvalidLayout)
    }

    pub(crate) fn sagitta_symbol_bytes_outdated(&self) -> bool {
        self.symbol_state
            .as_ref()
            .is_some_and(|state| state.source == SymbolPanelSource::Sagitta)
            && self
                .analysis_state
                .as_ref()
                .is_some_and(|state| state.validity == AnalysisValidity::OutdatedBytes)
    }

    pub(crate) fn mark_sagitta_edit_ops(&mut self, ops: &[EditOp]) {
        let Some(validity) = validity_for_edit_ops(ops) else {
            return;
        };
        self.mark_sagitta_validity(validity);
    }

    pub(crate) fn mark_sagitta_invalid_layout(&mut self) {
        self.mark_sagitta_validity(AnalysisValidity::InvalidLayout);
    }

    pub(crate) fn apply_sagitta_annotations(&self, rows: &mut [DisasmRow]) {
        let Some(state) = self.analysis_state.as_ref() else {
            return;
        };
        if !matches!(state.status, SagittaStatus::Ready)
            || state.validity == AnalysisValidity::InvalidLayout
        {
            return;
        }
        let Some(snapshot) = state.snapshot.as_ref() else {
            return;
        };

        let stale = state.validity == AnalysisValidity::OutdatedBytes;
        for row in rows {
            if let Some(va) = row.virtual_address {
                if let Some(function) = snapshot.function_at_entry(va) {
                    row.symbol_label = Some(function.name.clone());
                }
                if row.kind == DisasmRowKind::Instruction {
                    row.function_scope = snapshot.function_scope_for_row(va, row.len(), stale);
                }
            }
            if let Some(target) = row.direct_target.as_mut() {
                if let Some(function) = snapshot.function_at_entry(target.virtual_address) {
                    target.display_name = Some(function.name.clone());
                    if !row
                        .symbolized_names
                        .iter()
                        .any(|name| name == &function.name)
                    {
                        row.symbolized_names.push(function.name.clone());
                    }
                }
            }
        }
    }

    fn handle_sagitta_result(
        &mut self,
        job_id: u64,
        revision: u64,
        result: Result<SagittaSnapshot, String>,
    ) {
        if job_id != self.analysis_job_id {
            return;
        }

        match result {
            Ok(snapshot) => {
                let validity = if revision == self.document_revision {
                    AnalysisValidity::Current
                } else {
                    self.analysis_state
                        .as_ref()
                        .map_or(AnalysisValidity::OutdatedBytes, |state| state.validity)
                };
                let functions = snapshot.summary.functions;
                self.analysis_state = Some(SagittaAnalysisState {
                    status: SagittaStatus::Ready,
                    validity,
                    revision,
                    snapshot: Some(snapshot),
                });
                self.install_sagitta_symbol_panel_without_focus();
                match validity {
                    AnalysisValidity::Current => {
                        self.set_info_status(format!(
                            "Sagitta analysis ready ({functions} functions)"
                        ));
                    }
                    AnalysisValidity::OutdatedBytes => {
                        self.set_warning_status(format!(
                            "Sagitta analysis ready ({functions} functions); analysis outdated; rerun :ana"
                        ));
                    }
                    AnalysisValidity::InvalidLayout => {
                        self.set_warning_status(format!(
                            "Sagitta analysis ready ({functions} functions); analysis offsets changed; rerun :ana"
                        ));
                    }
                }
            }
            Err(message) => {
                self.analysis_state = Some(SagittaAnalysisState {
                    status: SagittaStatus::Failed(message.clone()),
                    validity: AnalysisValidity::Current,
                    revision,
                    snapshot: None,
                });
                self.set_error_status(format!("Sagitta analysis failed: {message}"));
            }
        }
    }

    fn install_sagitta_symbol_panel_without_focus(&mut self) {
        let Some(entries) = self
            .ready_sagitta_snapshot()
            .map(SagittaSnapshot::symbol_entries)
        else {
            return;
        };
        self.symbol_state = Some(SymbolState::from_entries(
            entries,
            SymbolPanelSource::Sagitta,
        ));
        if !self.show_side_panel {
            self.show_side_panel = true;
            self.active_side_panel = SidePanelKind::Symbol;
        } else if self.active_side_panel == SidePanelKind::Symbol {
            self.ensure_symbol_selection_visible();
        }
    }

    fn ready_sagitta_snapshot(&self) -> Option<&SagittaSnapshot> {
        let state = self.analysis_state.as_ref()?;
        matches!(state.status, SagittaStatus::Ready)
            .then_some(state.snapshot.as_ref())
            .flatten()
    }

    fn mark_sagitta_validity(&mut self, validity: AnalysisValidity) {
        let Some(state) = self.analysis_state.as_mut() else {
            return;
        };
        state.validity = match (state.validity, validity) {
            (AnalysisValidity::InvalidLayout, _) | (_, AnalysisValidity::InvalidLayout) => {
                AnalysisValidity::InvalidLayout
            }
            (_, AnalysisValidity::OutdatedBytes) => AnalysisValidity::OutdatedBytes,
            (current, AnalysisValidity::Current) => current,
        };
    }
}

impl SagittaSnapshot {
    fn function_at_entry(&self, va: u64) -> Option<&RecoveredFunction> {
        let idx = self
            .function_entry_index
            .partition_point(|(entry_va, _)| *entry_va < va);
        let (_, function_idx) = self
            .function_entry_index
            .get(idx)
            .filter(|(entry_va, _)| *entry_va == va)?;
        self.functions.get(*function_idx)
    }

    fn function_scope_for_row(
        &self,
        va: u64,
        row_len: usize,
        stale: bool,
    ) -> Option<DisasmFunctionScope> {
        let row_start = va;
        let row_end = va.saturating_add(row_len.max(1) as u64);
        let scope = self.scope_index_entry_for_va(va)?;
        let function = self.functions.get(scope.function_idx)?;
        let is_entry = function.entry_va >= row_start && function.entry_va < row_end;
        let is_exit = row_start < scope.function_end && row_end >= scope.function_end;
        let boundary = match (is_entry, is_exit) {
            (true, true) => DisasmFunctionBoundary::EntryExit,
            (true, false) => DisasmFunctionBoundary::Entry,
            (false, true) => DisasmFunctionBoundary::Exit,
            (false, false) => {
                if row_start >= scope.function_start && row_start < scope.function_end {
                    DisasmFunctionBoundary::Body
                } else {
                    return None;
                }
            }
        };
        Some(DisasmFunctionScope {
            name: function.name.clone(),
            entry_va: function.entry_va,
            boundary,
            stale,
        })
    }

    fn scope_index_entry_for_va(&self, va: u64) -> Option<&FunctionScopeIndexEntry> {
        let idx = self
            .function_scope_index
            .partition_point(|entry| entry.start_va <= va);
        let candidate_start = self.function_scope_index.get(idx.checked_sub(1)?)?.start_va;
        let mut best: Option<&FunctionScopeIndexEntry> = None;
        let mut idx = idx;
        while idx > 0 {
            idx -= 1;
            let entry = &self.function_scope_index[idx];
            if entry.start_va != candidate_start {
                break;
            }
            if entry.end_va > va {
                best = match best {
                    Some(current) if current.function_len() <= entry.function_len() => {
                        Some(current)
                    }
                    _ => Some(entry),
                };
            }
        }
        best
    }
}

impl FunctionScopeIndexEntry {
    fn function_len(&self) -> u64 {
        self.function_end.saturating_sub(self.function_start)
    }
}

fn build_function_entry_index(functions: &[RecoveredFunction]) -> Vec<(u64, usize)> {
    let mut index = functions
        .iter()
        .enumerate()
        .map(|(idx, function)| (function.entry_va, idx))
        .collect::<Vec<_>>();
    index.sort_unstable();
    index
}

fn build_function_scope_index(
    functions: &[RecoveredFunction],
    blocks: &[RecoveredBlock],
) -> Vec<FunctionScopeIndexEntry> {
    let block_by_start = blocks
        .iter()
        .map(|block| (block.start_va, block))
        .collect::<HashMap<_, _>>();
    let function_ranges = functions
        .iter()
        .map(|function| function_range_from_blocks(function, &block_by_start))
        .collect::<Vec<_>>();
    let mut index = Vec::new();
    for (function_idx, &(function_start, function_end)) in function_ranges.iter().enumerate() {
        index.push(FunctionScopeIndexEntry {
            start_va: function_start,
            end_va: function_end,
            function_start,
            function_end,
            function_idx,
        });
    }
    index.sort_unstable_by_key(|entry| (entry.start_va, entry.end_va, entry.function_idx));
    index
}

fn function_range_from_blocks(
    function: &RecoveredFunction,
    block_by_start: &HashMap<u64, &RecoveredBlock>,
) -> (u64, u64) {
    let mut start = None;
    let mut end = None;
    for block_start in &function.blocks {
        let Some(block) = block_by_start.get(block_start) else {
            continue;
        };
        let block_end = block.end_va.max(block.start_va.saturating_add(1));
        start = Some(start.map_or(block.start_va, |current: u64| current.min(block.start_va)));
        end = Some(end.map_or(block_end, |current: u64| current.max(block_end)));
    }
    match (start, end) {
        (Some(start), Some(end)) => (start, end),
        _ => (function.entry_va, function.entry_va.saturating_add(1)),
    }
}

fn materialize_logical_bytes(document: &mut Document) -> HxResult<Vec<u8>> {
    let visible_len = document.visible_len();
    if visible_len > SAGITTA_INPUT_LIMIT {
        return Err(HxError::CommandError(
            "Sagitta analysis input too large; limit is 128 MiB".to_owned(),
        ));
    }
    let mut bytes = Vec::with_capacity(visible_len as usize);
    if !document.is_empty() {
        document.for_each_logical_chunk(0, document.len() - 1, |chunk| {
            bytes.extend_from_slice(chunk);
            Ok(())
        })?;
    }
    Ok(bytes)
}

fn validate_sagitta_supported_input(bytes: &[u8]) -> HxResult<()> {
    if bytes.starts_with(b"\x7fELF") {
        return validate_sagitta_elf(bytes);
    }
    if bytes.starts_with(b"MZ") {
        return validate_sagitta_pe(bytes);
    }
    Err(HxError::CommandError("unsupported format".to_owned()))
}

fn validate_sagitta_elf(bytes: &[u8]) -> HxResult<()> {
    if bytes.len() < 20 {
        return Err(HxError::CommandError("unsupported format".to_owned()));
    }
    let class = bytes[4];
    let data = bytes[5];
    if !matches!(class, 1 | 2) || data != 1 {
        return Err(HxError::CommandError("unsupported format".to_owned()));
    }
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    if matches!(machine, 3 | 0x3e) {
        Ok(())
    } else {
        Err(HxError::CommandError("unsupported arch".to_owned()))
    }
}

fn validate_sagitta_pe(bytes: &[u8]) -> HxResult<()> {
    if bytes.len() < 0x40 {
        return Err(HxError::CommandError("unsupported format".to_owned()));
    }
    let pe_offset =
        u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
    let Some(coff) = bytes.get(pe_offset..pe_offset.saturating_add(6)) else {
        return Err(HxError::CommandError("unsupported format".to_owned()));
    };
    if &coff[0..4] != b"PE\0\0" {
        return Err(HxError::CommandError("unsupported format".to_owned()));
    }
    let machine = u16::from_le_bytes([coff[4], coff[5]]);
    if matches!(machine, 0x014c | 0x8664) {
        Ok(())
    } else {
        Err(HxError::CommandError("unsupported arch".to_owned()))
    }
}

fn analyze_sagitta_bytes(bytes: Vec<u8>) -> Result<SagittaSnapshot, String> {
    let analysis = sagitta::analyze_bytes(
        &bytes,
        sagitta::AnalysisConfig {
            depth: sagitta::AnalysisDepth::Indirects,
            ..Default::default()
        },
    )
    .map_err(|err| normalize_sagitta_error(&err.to_string()))?;
    Ok(snapshot_from_analysis(&analysis))
}

fn snapshot_from_analysis(analysis: &sagitta::Analysis) -> SagittaSnapshot {
    let functions = analysis
        .functions()
        .map(|function| {
            let entry_va = function.entry().get();
            let name = function
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("sub_{entry_va:x}"));
            let name_kind = if function.name().is_some() {
                SymbolNameKind::Real
            } else {
                SymbolNameKind::Synthetic
            };
            let provenance = function.provenance();
            RecoveredFunction {
                entry_va,
                entry_logical_offset: analysis
                    .va_to_file_offset(function.entry())
                    .map(|offset| offset.0),
                name,
                name_kind,
                confidence: provenance.confidence.into(),
                provenance: provenance
                    .sources
                    .into_iter()
                    .map(RecoveredFunctionSource::from)
                    .collect(),
                blocks: function.blocks().map(|addr| addr.get()).collect(),
                callers: function
                    .callers()
                    .into_iter()
                    .map(RecoveredCallRef::from)
                    .collect(),
                callees: function
                    .callees()
                    .into_iter()
                    .map(RecoveredCallRef::from)
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let blocks = analysis
        .blocks()
        .map(|block| RecoveredBlock {
            start_va: block.start().get(),
            end_va: block.end().get(),
            logical_offset: analysis
                .va_to_file_offset(block.start())
                .map(|offset| offset.0),
            size: block.size(),
        })
        .collect::<Vec<_>>();
    let cfg_edges = analysis
        .cfg_edges()
        .map(|edge| RecoveredCfgEdge {
            src_va: edge.src.get(),
            dst_va: edge.dst.get(),
            kind: format!("{:?}", edge.kind),
        })
        .collect::<Vec<_>>();
    let call_edges = analysis
        .call_edges()
        .map(|edge| RecoveredCallEdge {
            caller_va: edge.caller.get(),
            callee_va: edge.callee.get(),
            kind: format!("{:?}", edge.kind),
        })
        .collect::<Vec<_>>();
    let diagnostics = analysis
        .diagnostics()
        .map(|diagnostic| RecoveredDiagnostic {
            va: diagnostic.at.get(),
            severity: format!("{:?}", diagnostic.severity),
            message: diagnostic.message,
        })
        .collect::<Vec<_>>();
    SagittaSnapshot::new(
        SagittaSummary {
            functions: functions.len(),
            blocks: blocks.len(),
            cfg_edges: cfg_edges.len(),
            call_edges: call_edges.len(),
            diagnostics: diagnostics.len(),
        },
        functions,
        blocks,
        cfg_edges,
        call_edges,
        diagnostics,
    )
}

fn validity_for_edit_ops(ops: &[EditOp]) -> Option<AnalysisValidity> {
    let mut validity = None;
    for op in ops {
        match op {
            EditOp::Insert { cells, .. } | EditOp::RealDelete { cells, .. }
                if !cells.is_empty() =>
            {
                return Some(AnalysisValidity::InvalidLayout);
            }
            EditOp::TombstoneDelete { ids } if !ids.is_empty() => {
                return Some(AnalysisValidity::InvalidLayout);
            }
            EditOp::ReplaceBytes { changes }
                if changes.iter().any(|change| change.before != change.after) =>
            {
                validity = Some(AnalysisValidity::OutdatedBytes);
            }
            _ => {}
        }
    }
    validity
}

fn normalize_sagitta_error(message: &str) -> String {
    if message.contains("unsupported") && message.contains("arch") {
        "unsupported arch".to_owned()
    } else if message.contains("unsupported")
        || message.contains("load")
        || message.contains("parse")
    {
        "unsupported format".to_owned()
    } else {
        message.to_owned()
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        format!("Sagitta panic: {message}")
    } else if let Some(message) = payload.downcast_ref::<String>() {
        format!("Sagitta panic: {message}")
    } else {
        "Sagitta panic".to_owned()
    }
}

fn confidence_label(confidence: RecoveredConfidence) -> &'static str {
    match confidence {
        RecoveredConfidence::Pinned => "pinned",
        RecoveredConfidence::Structural => "structural",
        RecoveredConfidence::Heuristic => "heuristic",
        RecoveredConfidence::AiProposed => "ai-proposed",
    }
}

impl From<sagitta::Confidence> for RecoveredConfidence {
    fn from(confidence: sagitta::Confidence) -> Self {
        match confidence {
            sagitta::Confidence::Pinned => Self::Pinned,
            sagitta::Confidence::Structural => Self::Structural,
            sagitta::Confidence::Heuristic => Self::Heuristic,
            sagitta::Confidence::AiProposed => Self::AiProposed,
        }
    }
}

impl From<sagitta::FunctionSeedSource> for RecoveredFunctionSource {
    fn from(source: sagitta::FunctionSeedSource) -> Self {
        match source {
            sagitta::FunctionSeedSource::Entry => Self::Entry,
            sagitta::FunctionSeedSource::Symbol => Self::Symbol,
            sagitta::FunctionSeedSource::DirectCall => Self::DirectCall,
            sagitta::FunctionSeedSource::PltStub => Self::PltStub,
            sagitta::FunctionSeedSource::ThunkTarget => Self::ThunkTarget,
            sagitta::FunctionSeedSource::ElfLibcStartMain => Self::ElfLibcStartMain,
            sagitta::FunctionSeedSource::InitArray => Self::InitArray,
            sagitta::FunctionSeedSource::FiniArray => Self::FiniArray,
            sagitta::FunctionSeedSource::PeExport => Self::PeExport,
            sagitta::FunctionSeedSource::PeTlsCallback => Self::PeTlsCallback,
            sagitta::FunctionSeedSource::PeEntryThunk => Self::PeEntryThunk,
            sagitta::FunctionSeedSource::CetPrologue => Self::CetPrologue,
            sagitta::FunctionSeedSource::ElfInitSection => Self::ElfInitSection,
            sagitta::FunctionSeedSource::ElfFiniSection => Self::ElfFiniSection,
            sagitta::FunctionSeedSource::ElfFde => Self::ElfFde,
            sagitta::FunctionSeedSource::PeRuntimeFunction => Self::PeRuntimeFunction,
            sagitta::FunctionSeedSource::External => Self::External,
            sagitta::FunctionSeedSource::GoPclntab => Self::GoPclntab,
        }
    }
}

impl From<sagitta::FunctionCallReference> for RecoveredCallRef {
    fn from(reference: sagitta::FunctionCallReference) -> Self {
        Self {
            function_va: reference.function.get(),
            kind: format!("{:?}", reference.kind),
            sites: reference.sites.into_iter().map(|site| site.get()).collect(),
        }
    }
}
