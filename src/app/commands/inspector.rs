use super::*;

impl App {
    pub(super) fn execute_inspector_command(&mut self) {
        let from_inspector = self.active_side_panel == SidePanelKind::Inspector
            && self
                .command_return_mode
                .is_some_and(|mode| mode.is_side_panel());
        if !self.show_side_panel || self.active_side_panel != SidePanelKind::Inspector {
            self.show_side_panel = true;
            self.ensure_inspector_page_state(true);
            self.focus_inspector_page_or_warn();
        } else if !from_inspector {
            self.ensure_inspector_page_state(true);
            self.focus_inspector_page_or_warn();
        } else {
            self.mode = Mode::Normal;
            self.show_side_panel = false;
        }
    }

    pub(super) fn execute_inspector_more_command(&mut self) {
        if !self.show_side_panel
            || self.active_side_panel != SidePanelKind::Inspector
            || self.inspector().is_none()
        {
            self.set_warning_status("inspector not active; run `:insp` first");
            return;
        }
        let before = self.inspector_entry_cap;
        let after = before.saturating_add(crate::format::detect::DEFAULT_ENTRY_CAP);
        self.inspector_entry_cap = after;
        self.refresh_inspector();
        let more_pending = self
            .inspector()
            .map(|state| has_pending_more_marker(&state.structs))
            .unwrap_or(false);
        if more_pending {
            self.set_info_status(format!(
                "inspector cap raised to {after}; more entries still pending"
            ));
        } else {
            self.set_info_status(format!(
                "inspector cap raised to {after}; all entries loaded"
            ));
        }
    }

    pub(super) fn execute_format_command(&mut self, name: Option<String>) {
        self.show_side_panel = true;
        self.activate_inspector_page();
        // Reset pagination on explicit format switches so a leftover high cap
        // from a previous format doesn't silently over-parse the new one.
        self.inspector_entry_cap = crate::format::detect::DEFAULT_ENTRY_CAP;
        match name {
            Some(name) => self.execute_named_format_command(name),
            None => {
                self.inspector_format_override = None;
                self.inspector_state = None;
                self.inspector_error = None;
                self.refresh_inspector();
                if self.focus_inspector_page_or_warn() {
                    self.set_info_status("format: auto");
                }
            }
        }
    }

    pub(super) fn execute_named_format_command(&mut self, name: String) {
        if crate::format::detect::detect_by_name(&name, &mut self.document).is_some() {
            self.inspector_format_override = Some(name.to_lowercase());
            self.inspector_state = None;
            self.inspector_error = None;
            self.activate_inspector_page();
            self.refresh_inspector();
            if self.focus_inspector_page_or_warn() {
                self.set_info_status(format!("format: {}", name));
            }
        } else {
            self.set_error_status(format!("unknown or mismatched format: {}", name));
        }
    }
}

fn has_pending_more_marker(structs: &[StructValue]) -> bool {
    structs.iter().any(|structure| {
        (structure.name.starts_with('…') && structure.name.contains("more"))
            || has_pending_more_marker(&structure.children)
    })
}
