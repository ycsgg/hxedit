use super::*;

impl App {
    #[cfg(feature = "symbols")]
    pub(super) fn execute_symbols_command(&mut self) -> HxResult<()> {
        // Need ExecutableInfo to display symbols
        let info = match &self.main_view {
            crate::app::MainView::Disassembly(state) => state.info.clone(),
            _ => detect_executable_info(&mut self.document)
                .ok_or_else(|| HxError::CommandError("no executable format detected".to_owned()))?,
        };

        if info.symbols_by_va.is_empty() && info.target_names_by_va.is_empty() {
            return Err(HxError::CommandError("no symbols found".to_owned()));
        }

        // Create symbol panel state
        let count = info.symbols_by_va.len() + info.target_names_by_va.len();
        self.symbol_state = Some(SymbolState {
            info,
            scroll_offset: 0,
            selected_row: 0,
            detail_scroll_offset: 0,
        });
        self.show_side_panel = true;
        self.focus_symbol_panel();
        self.set_info_status(format!("symbol view ({count} symbols)"));
        Ok(())
    }

    #[cfg(feature = "symbols")]
    pub(super) fn execute_symbols_off_command(&mut self) {
        self.symbol_state = None;
        // Try to restore inspector
        self.restore_inspector_after_side_panel_close();
        if self.inspector().is_some() || self.inspector_error.is_some() {
            self.show_side_panel = true;
            if self.mode.is_side_panel() {
                self.mode = Mode::SidePanel;
            }
            self.set_info_status("symbol view off");
        } else {
            // No format to display, close panel
            self.show_side_panel = false;
            if self.mode.is_side_panel() {
                self.mode = Mode::Normal;
            }
            self.set_info_status("symbol view off (no format detected)");
        }
    }
}
