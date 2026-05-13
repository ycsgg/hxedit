use super::*;

impl App {
    pub(super) fn execute_search_command(
        &mut self,
        kind: SearchKind,
        pattern: Vec<u8>,
        backward: bool,
    ) -> HxResult<()> {
        let search = SearchState {
            kind,
            query: crate::app::SearchQuery::Bytes(pattern),
        };
        self.last_search = Some(search.clone());
        self.run_search(&search, search_direction(backward))
    }

    #[cfg(feature = "disasm")]
    pub(super) fn execute_instruction_search_command(
        &mut self,
        pattern: String,
        backward: bool,
    ) -> HxResult<()> {
        let search = SearchState {
            kind: SearchKind::Instruction,
            query: crate::app::SearchQuery::Instruction(pattern.to_ascii_lowercase()),
        };
        self.last_search = Some(search.clone());
        self.run_search(&search, search_direction(backward))
    }

    #[cfg(feature = "symbols")]
    pub(super) fn execute_symbol_search_command(
        &mut self,
        pattern: String,
        backward: bool,
    ) -> HxResult<()> {
        let search = SearchState {
            kind: SearchKind::Symbol,
            query: crate::app::SearchQuery::Symbol(pattern.to_ascii_lowercase()),
        };
        self.last_search = Some(search.clone());
        self.run_search(&search, search_direction(backward))
    }

    #[cfg(feature = "disasm")]
    pub(super) fn execute_disassemble_command(&mut self, arch: Option<&str>) -> HxResult<()> {
        let mut info = detect_executable_info(&mut self.document).ok_or_else(|| {
            crate::error::HxError::DisassemblyUnavailable(
                "not a recognized executable container".to_owned(),
            )
        })?;
        if let Some(raw_arch) = arch {
            info = override_arch(&info, raw_arch)?;
        }
        let backend = resolve_backend_kind(&info, None)?;
        let target = if info.span_containing(self.cursor).is_some() {
            self.cursor
        } else if let Some(entry) = info.entry_offset {
            entry
        } else if let Some(span) = info.first_executable_span() {
            span.start
        } else {
            0
        };
        self.enter_disassembly_view(info, backend, target)
    }

    #[cfg(feature = "disasm")]
    pub(super) fn execute_disassemble_force_command(
        &mut self,
        arch: &str,
        offset: u64,
    ) -> HxResult<()> {
        let info = force_raw_executable_info(self.document.len(), arch, offset)?;
        let backend = resolve_backend_kind(&info, None)?;
        self.enter_disassembly_view(info, backend, offset)
    }

    #[cfg(feature = "disasm")]
    pub(super) fn execute_disassemble_off_command(&mut self) {
        self.main_view = crate::app::MainView::Hex;
        self.clear_disassembly_runtime();
        self.set_info_status("disassembly off");
    }

    #[cfg(feature = "disasm")]
    fn enter_disassembly_view(
        &mut self,
        info: crate::executable::ExecutableInfo,
        backend: crate::disasm::BackendKind,
        target: u64,
    ) -> HxResult<()> {
        self.cursor = self.clamp_offset(target);
        self.main_view = crate::app::MainView::Disassembly(DisassemblyState::new(
            info.clone(),
            backend,
            self.cursor,
        ));
        self.reset_disassembly_runtime()?;
        self.focus_disassembly_row_at_offset(self.cursor)?;
        let metadata_suffix = match (info.symbol_count(), info.import_count()) {
            (0, 0) => String::new(),
            (symbols, 0) => format!(" [{symbols} syms]"),
            (0, imports) => format!(" [{imports} imports]"),
            (symbols, imports) => format!(" [{symbols} syms, {imports} imports]"),
        };
        self.set_info_status(format!(
            "disassembly: {} {} ({}, {}) @ 0x{:x}{}",
            info.kind.label(),
            info.arch.label(),
            info.bitness.label(),
            backend.label(),
            self.cursor,
            metadata_suffix
        ));
        Ok(())
    }
}

fn search_direction(backward: bool) -> SearchDirection {
    if backward {
        SearchDirection::Backward
    } else {
        SearchDirection::Forward
    }
}
