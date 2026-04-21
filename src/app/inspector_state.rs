use crate::app::{App, InspectorState};
use crate::error::{HxError, HxResult};
use crate::format;
use crate::format::parse::{InspectorRow, NodePath, StructValue};
use crate::format::types::FieldDef;
use crate::mode::Mode;
use crate::view::inspector as inspector_view;
use crate::view::layout::MIN_INSPECTOR_WIDTH;

/// Structs at this depth (and deeper) are collapsed on first build. `1` keeps
/// top-level structs expanded so the user sees where the file starts, but
/// hides Program Headers / nested sections behind a `▶` until they click in.
const DEFAULT_COLLAPSED_DEPTH: usize = 1;

impl App {
    fn supported_inspector_formats() -> &'static str {
        "ELF / PNG / ZIP / GZIP"
    }

    pub(crate) fn inspector_has_editable_fields(&self) -> bool {
        self.inspector
            .as_ref()
            .map(|inspector| {
                inspector
                    .rows
                    .iter()
                    .any(|row| matches!(row, InspectorRow::Field { editable: true, .. }))
            })
            .unwrap_or(false)
    }

    fn no_format_detected_message(&self) -> String {
        format!(
            "inspector unavailable: no format detected (supported: {}; use :format to force)",
            Self::supported_inspector_formats()
        )
    }

    fn current_main_inner_width(&self) -> Option<u16> {
        self.last_columns.map(|columns| {
            columns.gutter.width
                + columns.sep1.width
                + columns.hex.width
                + columns.sep2.width
                + columns.ascii.width
                + columns.sep3.map(|area| area.width).unwrap_or(0)
                + columns.inspector.map(|area| area.width).unwrap_or(0)
        })
    }

    fn warn_inspector_too_narrow(&mut self) {
        match self.current_main_inner_width() {
            Some(current) => self.set_warning_status(format!(
                "inspector hidden; terminal too narrow (current {} columns, need {}+)",
                current, MIN_INSPECTOR_WIDTH
            )),
            None => self.set_warning_status(format!(
                "inspector hidden; terminal too narrow (need {}+ columns)",
                MIN_INSPECTOR_WIDTH
            )),
        }
    }

    pub(crate) fn inspector_panel_visible(&self) -> bool {
        self.current_main_inner_width()
            .map(|width| width >= MIN_INSPECTOR_WIDTH)
            .unwrap_or(true)
    }

    pub(crate) fn ensure_inspector_mode_visible(&mut self) {
        if !self.mode.is_inspector() || self.inspector_panel_visible() {
            return;
        }
        if let Some(inspector) = self.inspector.as_mut() {
            inspector.editing = None;
        }
        self.mode = Mode::Normal;
        self.warn_inspector_too_narrow();
    }

    pub(crate) fn focus_inspector_or_warn(&mut self) -> bool {
        self.focus_inspector_or_warn_with_toggle(false)
    }

    fn focus_inspector_or_warn_with_toggle(&mut self, is_toggle_attempt: bool) -> bool {
        if !self.inspector_panel_visible() {
            if let Some(inspector) = self.inspector.as_mut() {
                inspector.editing = None;
            }
            self.mode = Mode::Normal;
            self.warn_inspector_too_narrow();
            return false;
        }
        if let Some(error) = self.inspector_error.as_ref() {
            self.mode = Mode::Normal;
            self.set_error_status(error.clone());
            return false;
        }
        if self.inspector.is_none() {
            // 没有检测到格式
            if is_toggle_attempt {
                // 用户再次按 Tab/`:insp` 尝试切换，关闭 inspector
                self.mode = Mode::Normal;
                self.show_inspector = false;
                self.clear_status();
                return false;
            }
            self.mode = Mode::Normal;
            self.set_warning_status(self.no_format_detected_message());
            return false;
        }
        self.mode = Mode::Inspector;
        self.sync_inspector_to_cursor();
        if !self.inspector_has_editable_fields() {
            let format_name = self
                .inspector
                .as_ref()
                .map(|inspector| inspector.format_name.clone())
                .unwrap_or_else(|| "current".to_owned());
            self.set_info_status(format!("{format_name} inspector is view-only"));
        }
        true
    }

    pub(crate) fn inspector_edit_warning(&self) -> Option<&'static str> {
        match self.inspector.as_ref()?.format_name.as_str() {
            "PNG" => Some("PNG inspector edits do not repair CRC or chunk consistency"),
            "ZIP" => Some("ZIP inspector edits do not repair header or descriptor consistency"),
            "GZIP" => Some("GZIP inspector edits do not recompute header/trailer consistency"),
            _ => None,
        }
    }

    pub(crate) fn toggle_inspector_mode(&mut self) {
        if !self.show_inspector {
            self.show_inspector = true;
            self.refresh_inspector();
            self.focus_inspector_or_warn_with_toggle(false);
        } else if !self.mode.is_inspector() {
            self.focus_inspector_or_warn_with_toggle(true);
        } else {
            if let Some(inspector) = self.inspector.as_mut() {
                inspector.editing = None;
            }
            self.mode = Mode::Normal;
            self.show_inspector = false;
            self.inspector = None;
            self.inspector_error = None;
        }
    }

    pub(crate) fn inspector_visible_rows(&self) -> usize {
        self.view_rows.saturating_sub(1).max(1)
    }

    pub(crate) fn current_inspector_width(&self) -> u16 {
        self.last_columns
            .and_then(|columns| columns.inspector)
            .map(|area| area.width)
            .unwrap_or(32)
    }

    pub(crate) fn inspector_rendered_lines(
        &self,
        width: u16,
    ) -> Vec<inspector_view::RenderedInspectorLine> {
        let Some(inspector) = self.inspector.as_ref() else {
            return Vec::new();
        };
        let editing = inspector
            .editing
            .as_ref()
            .map(|edit| (edit.buffer.as_str(), edit.cursor_pos));
        inspector_view::build_wrapped(
            &inspector.rows,
            inspector.selected_row,
            editing,
            width,
            &self.palette,
        )
    }

    pub(crate) fn inspector_highlight_range(&self) -> Option<(u64, u64)> {
        let inspector = self.inspector.as_ref()?;
        match inspector.rows.get(inspector.selected_row)? {
            InspectorRow::Field {
                abs_offset, size, ..
            } if *size > 0 => Some((*abs_offset, abs_offset + *size as u64 - 1)),
            _ => None,
        }
    }

    /// Re-run format detection/parsing and refresh the inspector panel.
    pub(crate) fn refresh_inspector(&mut self) {
        if !self.show_inspector {
            return;
        }

        let previous_scroll = self
            .inspector
            .as_ref()
            .map(|state| state.scroll_offset)
            .unwrap_or(0);
        let previous_selected_offset = self
            .inspector
            .as_ref()
            .and_then(|state| field_offset_for_row(&state.rows, state.selected_row));
        let previous_collapsed = self
            .inspector
            .as_ref()
            .map(|state| state.collapsed_nodes.clone());
        let previous_format = self
            .inspector
            .as_ref()
            .map(|state| state.format_name.clone());

        let detected = if let Some(name) = self.inspector_format_override.as_deref() {
            format::detect::detect_by_name_with_cap(
                name,
                &mut self.document,
                self.inspector_entry_cap,
            )
        } else {
            format::detect::detect_format_with_cap(&mut self.document, self.inspector_entry_cap)
        };

        if let Some(def) = detected {
            match format::parse::parse_format(&def, &mut self.document) {
                Ok(structs) => {
                    let collapsed_nodes = previous_collapsed.unwrap_or_else(|| {
                        format::parse::initial_collapsed_nodes(&structs, DEFAULT_COLLAPSED_DEPTH)
                    });
                    let rows = format::parse::flatten(&structs, &collapsed_nodes);
                    let selected_row = previous_selected_offset
                        .and_then(|offset| find_row_covering_offset(&rows, offset))
                        .or_else(|| find_row_covering_offset(&rows, self.cursor))
                        .unwrap_or_else(|| first_selectable_row(&rows));

                    self.inspector = Some(InspectorState {
                        format_name: def.name,
                        structs,
                        rows,
                        scroll_offset: previous_scroll,
                        selected_row,
                        editing: None,
                        collapsed_nodes,
                    });
                    self.inspector_error = None;
                    self.ensure_inspector_selection_visible();
                }
                Err(err) => {
                    let message = format!("inspector parse failed [{}]: {}", def.name, err);
                    if self.inspector_error.as_deref() != Some(message.as_str()) {
                        eprintln!("{message}");
                    }
                    self.inspector = None;
                    self.inspector_error = Some(message.clone());
                    self.set_error_status(message);
                    if matches!(self.mode, Mode::InspectorEdit) {
                        self.mode = Mode::Inspector;
                    }
                }
            }
        } else {
            self.inspector = None;
            self.inspector_error = None;
            // When detection previously succeeded and now fails, it's almost
            // always because the user just overwrote part of the magic or
            // header. Surfacing this makes it obvious why the panel suddenly
            // went blank.
            if let Some(prev_format) = previous_format {
                self.set_warning_status(format!(
                    "format lost: {} header/magic no longer matches",
                    prev_format
                ));
                if matches!(self.mode, Mode::InspectorEdit | Mode::Inspector) {
                    self.mode = Mode::Normal;
                }
            }
        }
    }

    pub(crate) fn inspector_empty_panel_message(&self) -> String {
        self.no_format_detected_message()
    }

    /// Sync inspector selection to the current hex cursor when hex has focus.
    pub(crate) fn sync_inspector_to_cursor(&mut self) {
        if self.mode.is_inspector() {
            return;
        }
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        if let Some(row) = find_row_covering_offset(&inspector.rows, self.cursor) {
            inspector.selected_row = row;
            self.ensure_inspector_selection_visible();
        }
    }

    /// Move the hex cursor to the currently selected inspector row.
    ///
    /// For field rows this lands on the field's absolute offset; for header
    /// rows (when a collapsible header is selected) the cursor moves to the
    /// struct's `base_offset` so the user's eye tracks the block boundary.
    pub(crate) fn sync_cursor_to_inspector(&mut self) {
        self.ensure_inspector_selection_visible();
        let Some(inspector) = self.inspector.as_ref() else {
            return;
        };
        let Some(row) = inspector.rows.get(inspector.selected_row) else {
            return;
        };
        let abs_offset = match row {
            InspectorRow::Field { abs_offset, .. } => *abs_offset,
            InspectorRow::Header {
                node_path,
                has_children: true,
                ..
            } => match base_offset_for_path(&inspector.structs, node_path) {
                Some(off) => off,
                None => return,
            },
            InspectorRow::Header { .. } => return,
        };
        self.cursor = abs_offset;
        self.ensure_cursor_visible();
    }

    /// Ensure the selected inspector row stays within the visible panel window.
    pub(crate) fn ensure_inspector_selection_visible(&mut self) {
        let visible_rows = self.inspector_visible_rows();
        let width = self.current_inspector_width();
        let rendered = self.inspector_rendered_lines(width);
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let first_line = rendered
            .iter()
            .position(|line| line.row_index == inspector.selected_row)
            .unwrap_or(0);
        let last_line = rendered
            .iter()
            .rposition(|line| line.row_index == inspector.selected_row)
            .unwrap_or(first_line);
        if first_line < inspector.scroll_offset {
            inspector.scroll_offset = first_line;
        } else if last_line >= inspector.scroll_offset + visible_rows {
            inspector.scroll_offset = last_line.saturating_add(1).saturating_sub(visible_rows);
        }
        let max_scroll = rendered.len().saturating_sub(visible_rows);
        inspector.scroll_offset = inspector.scroll_offset.min(max_scroll);
    }

    pub(crate) fn scroll_inspector(&mut self, rows: i64) {
        let visible_rows = self.inspector_visible_rows();
        let width = self.current_inspector_width();
        let rendered_len = self.inspector_rendered_lines(width).len();
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let max_scroll = rendered_len.saturating_sub(visible_rows);
        inspector.scroll_offset = if rows >= 0 {
            inspector
                .scroll_offset
                .saturating_add(rows as usize)
                .min(max_scroll)
        } else {
            inspector
                .scroll_offset
                .saturating_sub(rows.unsigned_abs() as usize)
        };
    }

    /// Select a row in the inspector, preferring selectable rows (fields and
    /// collapsible headers) over decorative headers.
    pub(crate) fn set_inspector_selected_row(&mut self, target_row: usize) {
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let Some(row) = nearest_selectable_row(&inspector.rows, target_row) else {
            return;
        };
        inspector.selected_row = row;
        inspector.editing = None;
        self.ensure_inspector_selection_visible();
    }

    /// Toggle collapse state of the currently selected collapsible header row.
    /// No-op when the selection is on a field or a non-collapsible header.
    pub(crate) fn toggle_inspector_collapse(&mut self) {
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let Some(row) = inspector.rows.get(inspector.selected_row) else {
            return;
        };

        let node_path = match row {
            InspectorRow::Header {
                node_path,
                has_children: true,
                ..
            } => node_path.clone(),
            _ => return,
        };

        if !inspector.collapsed_nodes.insert(node_path.clone()) {
            inspector.collapsed_nodes.remove(&node_path);
        }

        inspector.rows = format::parse::flatten(&inspector.structs, &inspector.collapsed_nodes);
        inspector.editing = None;

        if let Some(new_pos) = inspector.rows.iter().position(
            |r| matches!(r, InspectorRow::Header { node_path: np, .. } if np == &node_path),
        ) {
            inspector.selected_row = new_pos;
        } else {
            inspector.selected_row = first_selectable_row(&inspector.rows);
        }

        self.ensure_inspector_selection_visible();
        self.sync_cursor_to_inspector();
    }

    /// Commit the current inspector edit back into the document.
    pub(crate) fn submit_inspector_edit(&mut self) -> HxResult<()> {
        let (row_index, buffer) = {
            let inspector = self.inspector.as_mut().ok_or(HxError::OffsetOutOfRange)?;
            let edit = inspector.editing.take().ok_or(HxError::OffsetOutOfRange)?;
            (edit.row_index, edit.buffer)
        };

        let row = self
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.rows.get(row_index))
            .cloned()
            .ok_or(HxError::OffsetOutOfRange)?;

        let (field_index, abs_offset, size) = match row {
            InspectorRow::Field {
                field_index,
                abs_offset,
                size,
                ..
            } => (field_index, abs_offset, size),
            InspectorRow::Header { .. } => return Ok(()),
        };

        let field_def = self
            .find_field_def(field_index)
            .ok_or(HxError::OffsetOutOfRange)?;
        let bytes = format::edit::encode_value(&field_def.field_type, &buffer)
            .map_err(HxError::InvalidOffset)?;
        if bytes.len() != size {
            return Err(HxError::InvalidOffset(format!(
                "expected {} bytes, got {}",
                size,
                bytes.len()
            )));
        }

        let ops = format::edit::write_field(&mut self.document, abs_offset, &bytes)?;
        let cursor_before = self.cursor;
        let mode_before = self.mode;

        self.refresh_inspector();
        self.mode = Mode::Inspector;
        self.sync_cursor_to_inspector();
        if !ops.is_empty() {
            self.push_undo_step(ops, cursor_before, mode_before, self.cursor, self.mode);
        }
        if let Some(warning) = self.inspector_edit_warning() {
            self.set_warning_status(format!("edited field at 0x{:x}; {}", abs_offset, warning));
        } else {
            self.set_info_status(format!("edited field at 0x{:x}", abs_offset));
        }
        Ok(())
    }

    pub(crate) fn inspector_read_only_message(
        &self,
        format_name: &str,
        field_name: &str,
    ) -> String {
        format!("{format_name} field '{field_name}' is read-only in inspector")
    }

    pub(crate) fn find_field_def(&self, field_index: usize) -> Option<FieldDef> {
        fn walk(
            structs: &[StructValue],
            field_index: usize,
            current: &mut usize,
        ) -> Option<FieldDef> {
            for sv in structs {
                for fv in &sv.fields {
                    if *current == field_index {
                        return Some(fv.def.clone());
                    }
                    *current += 1;
                }
                if let Some(found) = walk(&sv.children, field_index, current) {
                    return Some(found);
                }
            }
            None
        }

        let inspector = self.inspector.as_ref()?;
        let mut current = 0;
        walk(&inspector.structs, field_index, &mut current)
    }
}

fn first_selectable_row(rows: &[InspectorRow]) -> usize {
    rows.iter().position(is_selectable).unwrap_or(0)
}

fn field_offset_for_row(rows: &[InspectorRow], row_index: usize) -> Option<u64> {
    match rows.get(row_index) {
        Some(InspectorRow::Field { abs_offset, .. }) => Some(*abs_offset),
        _ => None,
    }
}

fn find_row_covering_offset(rows: &[InspectorRow], offset: u64) -> Option<usize> {
    rows.iter().position(|row| match row {
        InspectorRow::Field {
            abs_offset, size, ..
        } => offset >= *abs_offset && offset < abs_offset.saturating_add(*size as u64),
        InspectorRow::Header { .. } => false,
    })
}

fn nearest_selectable_row(rows: &[InspectorRow], target_row: usize) -> Option<usize> {
    let target_row = target_row.min(rows.len().saturating_sub(1));
    if rows.get(target_row).is_some_and(is_selectable) {
        return Some(target_row);
    }
    if let Some(row) = (target_row..rows.len()).find(|&r| rows.get(r).is_some_and(is_selectable)) {
        return Some(row);
    }
    (0..target_row)
        .rev()
        .find(|&r| rows.get(r).is_some_and(is_selectable))
}

/// A row is user-selectable when it represents actionable content: a field,
/// or a collapsible header that toggles child visibility.
pub(crate) fn is_selectable(row: &InspectorRow) -> bool {
    matches!(
        row,
        InspectorRow::Field { .. }
            | InspectorRow::Header {
                has_children: true,
                ..
            }
    )
}

/// Look up the `base_offset` of the struct at `target_path`.
///
/// Uses the same `(name, sibling_index)` accounting as `flatten`, so this
/// traversal matches the paths emitted into the row list.
fn base_offset_for_path(structs: &[StructValue], target_path: &NodePath) -> Option<u64> {
    use std::collections::HashMap;

    fn visit(
        sv: &StructValue,
        parent_path: &NodePath,
        sibling_index: usize,
        target: &NodePath,
    ) -> Option<u64> {
        let mut path = parent_path.clone();
        path.push((sv.name.clone(), sibling_index));
        if &path == target {
            return Some(sv.base_offset);
        }
        let mut counts: HashMap<String, usize> = HashMap::new();
        for child in &sv.children {
            let entry = counts.entry(child.name.clone()).or_insert(0);
            let idx = *entry;
            *entry += 1;
            if let Some(off) = visit(child, &path, idx, target) {
                return Some(off);
            }
        }
        None
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    let root: NodePath = Vec::new();
    for sv in structs {
        let entry = counts.entry(sv.name.clone()).or_insert(0);
        let idx = *entry;
        *entry += 1;
        if let Some(off) = visit(sv, &root, idx, target_path) {
            return Some(off);
        }
    }
    None
}
