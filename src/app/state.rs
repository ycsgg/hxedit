use crate::app::{App, EditOp, InspectorState, ReplacementUndo};
use crate::error::{HxError, HxResult};
use crate::format;
use crate::format::parse::{InspectorRow, StructValue};
use crate::format::types::FieldDef;
use crate::mode::{Mode, NibblePhase, PendingInsert};
use crate::view::inspector as inspector_view;

impl App {
    /// Delete the byte at cursor (normal) or the entire selection (visual).
    /// Both use tombstone deletion — the cell keeps its display slot.
    pub(crate) fn delete_at_cursor_or_selection(&mut self) -> HxResult<()> {
        if matches!(self.mode, Mode::Visual) {
            return self.delete_selection();
        }
        self.delete_current()
    }

    /// Tombstone-delete the single byte at cursor.
    pub(crate) fn delete_current(&mut self) -> HxResult<()> {
        let Some(id) = self.document.delete_byte(self.cursor)? else {
            return Ok(());
        };
        self.push_undo_step(
            vec![EditOp::TombstoneDelete { ids: vec![id] }],
            self.cursor,
            self.mode,
        );
        self.refresh_inspector();
        self.status_message = format!("deleted 0x{:x}", self.cursor);
        Ok(())
    }

    /// Tombstone-delete every byte in the visual selection.
    pub(crate) fn delete_selection(&mut self) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return self.delete_current();
        };

        let mut ids = Vec::with_capacity((end - start + 1) as usize);
        for offset in start..=end {
            if let Some(id) = self.document.delete_byte(offset)? {
                ids.push(id);
            }
        }

        self.push_undo_step(vec![EditOp::TombstoneDelete { ids }], start, Mode::Visual);
        self.cursor = self.clamp_offset(start);
        self.selection_anchor = None;
        self.mode = Mode::Normal;
        self.refresh_inspector();
        self.status_message = format!("deleted selection {} bytes", end - start + 1);
        Ok(())
    }

    /// Process a hex nibble in edit (replace) mode.
    ///
    /// High nibble → sets the upper 4 bits, advances phase to Low.
    /// Low nibble  → sets the lower 4 bits, advances cursor to next byte,
    ///               resets phase to High.
    pub(crate) fn edit_nibble(&mut self, value: u8) -> HxResult<()> {
        let offset = self.cursor;
        let phase = match self.mode {
            Mode::EditHex { phase } => phase,
            _ => return Ok(()),
        };

        let previous = if offset < self.document.len() {
            self.document
                .cell_id_at(offset)
                .map(|id| (id, self.document.replacement_state(id)))
        } else {
            None
        };
        let id = self.document.replace_nibble(offset, phase, value)?;

        let after = self.document.replacement_state(id);
        let previous = previous
            .map(|(_, state)| state)
            .or_else(|| (offset < self.document.len()).then_some(None))
            .flatten();
        if after != previous {
            self.push_undo_step(
                vec![EditOp::ReplaceBytes {
                    changes: vec![ReplacementUndo { id, previous }],
                }],
                self.cursor,
                self.mode,
            );
        } else if offset == self.document.len().saturating_sub(1) && previous.is_none() {
            self.push_undo_step(vec![EditOp::Insert { offset, len: 1 }], offset, self.mode);
        }

        self.status_message = format!("edited 0x{:x}", offset);
        self.mode = match phase {
            NibblePhase::High => Mode::EditHex {
                phase: NibblePhase::Low,
            },
            NibblePhase::Low => {
                self.cursor = (offset + 1).min(self.document.len());
                Mode::EditHex {
                    phase: NibblePhase::High,
                }
            }
        };
        self.refresh_inspector();
        Ok(())
    }

    /// Process a hex nibble in insert mode.
    ///
    /// Two-keystroke protocol:
    /// - **No pending**: insert a new byte `0xN0` at cursor, set pending.
    ///   Cursor stays on the just-inserted byte so the user sees `a0`.
    /// - **Has pending**: fill in the low nibble (`a0` → `ab`), commit the
    ///   insert to the undo stack, advance cursor past the completed byte.
    pub(crate) fn insert_nibble(&mut self, value: u8) -> HxResult<()> {
        let pending = match self.mode {
            Mode::InsertHex { pending } => pending,
            _ => return Ok(()),
        };

        match pending {
            None => {
                let offset = self.cursor;
                self.document.insert_byte(offset, value << 4)?;
                // Keep cursor on the just-inserted byte so the user sees `a0`
                // and can type the second nibble.  Cursor advances only after
                // the low nibble is filled in (see the Some branch below).
                self.cursor = offset;
                self.mode = Mode::InsertHex {
                    pending: Some(PendingInsert {
                        offset,
                        high_nibble: value,
                    }),
                };
                self.refresh_inspector();
                self.status_message = format!("inserted 0x{:x}", offset);
            }
            Some(pending) => {
                self.document
                    .replace_display_byte(pending.offset, (pending.high_nibble << 4) | value)?;
                self.commit_pending_insert()?;
                // Now advance past the completed byte.
                self.cursor = pending.offset + 1;
                self.refresh_inspector();
                self.status_message = format!("inserted 0x{:x}", pending.offset);
            }
        }

        Ok(())
    }

    /// Backspace in insert mode — the only "real delete" interaction.
    ///
    /// - **With pending**: remove the just-inserted half-byte, no undo step
    ///   (the byte was never committed).
    /// - **Without pending**: real-delete the byte before cursor, push a
    ///   `RealDelete` undo step so it can be restored.
    pub(crate) fn edit_backspace(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::InsertHex { pending } => {
                if let Some(pending) = pending {
                    self.document.delete_range_real(pending.offset, 1)?;
                    self.cursor = pending.offset;
                    self.mode = Mode::InsertHex { pending: None };
                    self.refresh_inspector();
                    self.status_message = format!("deleted 0x{:x}", pending.offset);
                    return Ok(());
                }

                if self.cursor == 0 {
                    return Ok(());
                }

                let delete_offset = self.cursor - 1;
                let removed = self.document.delete_range_real(delete_offset, 1)?;
                self.push_undo_step(
                    vec![EditOp::RealDelete {
                        offset: delete_offset,
                        cells: removed,
                    }],
                    self.cursor,
                    Mode::InsertHex { pending: None },
                );
                self.cursor = delete_offset;
                self.refresh_inspector();
                self.status_message = format!("deleted 0x{:x}", delete_offset);
                Ok(())
            }
            Mode::EditHex { .. }
            | Mode::Normal
            | Mode::Visual
            | Mode::Command
            | Mode::Inspector
            | Mode::InspectorEdit => Ok(()),
        }
    }

    pub(crate) fn toggle_visual(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Normal => {
                self.selection_anchor = Some(self.cursor);
                self.mode = Mode::Visual;
            }
            Mode::EditHex { .. }
            | Mode::InsertHex { .. }
            | Mode::Command
            | Mode::Inspector
            | Mode::InspectorEdit => {}
        }
        Ok(())
    }

    pub(crate) fn clear_error_if_command_done(&mut self) {
        let is_error = self.status_message.starts_with("invalid")
            || self.status_message.starts_with("unknown")
            || self.status_message.starts_with("missing")
            || self.status_message.contains("outside");
        if !matches!(self.mode, Mode::Command) && is_error {
            self.status_message.clear();
        }
    }

    /// Leave the current mode (Esc handler).
    ///
    /// - Visual → Normal (clear selection)
    /// - Command → return mode
    /// - InsertHex → commit pending, then Normal
    /// - EditHex / Normal → Normal
    pub(crate) fn leave_mode(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Command => {
                let return_mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
                self.mode = self.normalize_mode(return_mode);
            }
            Mode::InsertHex { .. } => {
                self.commit_pending_insert()?;
                self.mode = Mode::Normal;
            }
            Mode::Inspector => {
                if let Some(inspector) = self.inspector.as_mut() {
                    inspector.editing = None;
                }
                self.mode = Mode::Normal;
            }
            Mode::InspectorEdit => {
                if let Some(inspector) = self.inspector.as_mut() {
                    inspector.editing = None;
                }
                self.mode = Mode::Inspector;
            }
            Mode::EditHex { .. } | Mode::Normal => {
                self.mode = Mode::Normal;
            }
        }
        Ok(())
    }

    /// Finalize a half-completed insert byte into the undo stack.
    ///
    /// Called when leaving insert mode, moving the cursor, entering command
    /// mode, or clicking the mouse.  The pending byte's low nibble `0` is
    /// fixated as a real value.
    pub(crate) fn commit_pending_insert(&mut self) -> HxResult<Option<u64>> {
        let pending = match self.mode {
            Mode::InsertHex {
                pending: Some(pending),
            } => pending,
            _ => return Ok(None),
        };

        self.push_undo_step(
            vec![EditOp::Insert {
                offset: pending.offset,
                len: 1,
            }],
            pending.offset,
            Mode::InsertHex { pending: None },
        );
        self.mode = Mode::InsertHex { pending: None };
        Ok(Some(pending.offset))
    }

    /// Undo a pending insert without pushing an undo step.
    ///
    /// Used when the user presses undo while a half-byte is pending: the
    /// byte is silently removed (it was never committed to the undo stack).
    pub(crate) fn undo_pending_insert(&mut self) -> HxResult<bool> {
        let pending = match self.mode {
            Mode::InsertHex {
                pending: Some(pending),
            } => pending,
            _ => return Ok(false),
        };

        self.document.delete_range_real(pending.offset, 1)?;
        self.cursor = pending.offset;
        self.mode = Mode::InsertHex { pending: None };
        self.status_message = "undid 1 action".to_owned();
        Ok(true)
    }

    /// Commit any pending insert before a cursor move or mode switch.
    pub(crate) fn ensure_insert_pending_committed(&mut self) -> HxResult<()> {
        if matches!(self.mode, Mode::InsertHex { .. }) {
            self.commit_pending_insert()?;
        }
        Ok(())
    }

    pub(crate) fn normalize_mode(&self, mode: Mode) -> Mode {
        match mode {
            Mode::Inspector | Mode::InspectorEdit if !self.show_inspector => Mode::Normal,
            Mode::InspectorEdit
                if self
                    .inspector
                    .as_ref()
                    .and_then(|inspector| inspector.editing.as_ref())
                    .is_none() =>
            {
                Mode::Inspector
            }
            other => other,
        }
    }

    pub(crate) fn selection_range(&self) -> Option<(u64, u64)> {
        let anchor = self.selection_anchor?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    pub(crate) fn toggle_inspector_mode(&mut self) {
        if !self.show_inspector {
            self.show_inspector = true;
            self.refresh_inspector();
            self.mode = Mode::Inspector;
        } else if !self.mode.is_inspector() {
            self.mode = Mode::Inspector;
            self.sync_inspector_to_cursor();
        } else {
            if let Some(inspector) = self.inspector.as_mut() {
                inspector.editing = None;
            }
            self.mode = Mode::Normal;
            self.show_inspector = false;
            self.inspector = None;
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

        let detected = if let Some(name) = self.inspector_format_override.as_deref() {
            format::detect::detect_by_name(name, &mut self.document)
        } else {
            format::detect::detect_format(&mut self.document)
        };

        if let Some(def) = detected {
            let structs = format::parse::parse_format(&def, &mut self.document).unwrap_or_default();
            let rows = format::parse::flatten(&structs);
            let selected_row = previous_selected_offset
                .and_then(|offset| find_row_covering_offset(&rows, offset))
                .or_else(|| find_row_covering_offset(&rows, self.cursor))
                .unwrap_or_else(|| first_field_row(&rows));

            self.inspector = Some(InspectorState {
                format_name: def.name,
                structs,
                rows,
                scroll_offset: previous_scroll,
                selected_row,
                editing: None,
            });
            self.ensure_inspector_selection_visible();
        } else {
            self.inspector = None;
        }
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

    /// Move the hex cursor to the currently selected inspector field.
    pub(crate) fn sync_cursor_to_inspector(&mut self) {
        self.ensure_inspector_selection_visible();
        let Some(inspector) = self.inspector.as_ref() else {
            return;
        };
        if let Some(InspectorRow::Field { abs_offset, .. }) =
            inspector.rows.get(inspector.selected_row)
        {
            self.cursor = *abs_offset;
            self.ensure_cursor_visible();
        }
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

    /// Select a row in the inspector, preferring field rows over headers.
    pub(crate) fn set_inspector_selected_row(&mut self, target_row: usize) {
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let Some(row) = nearest_field_row(&inspector.rows, target_row) else {
            return;
        };
        inspector.selected_row = row;
        inspector.editing = None;
        self.ensure_inspector_selection_visible();
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
        if !ops.is_empty() {
            self.push_undo_step(ops, self.cursor, self.mode);
        }

        self.refresh_inspector();
        self.mode = Mode::Inspector;
        self.sync_cursor_to_inspector();
        self.status_message = format!("edited field at 0x{:x}", abs_offset);
        Ok(())
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

fn first_field_row(rows: &[InspectorRow]) -> usize {
    rows.iter()
        .position(|row| matches!(row, InspectorRow::Field { .. }))
        .unwrap_or(0)
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

fn nearest_field_row(rows: &[InspectorRow], target_row: usize) -> Option<usize> {
    let target_row = target_row.min(rows.len().saturating_sub(1));
    if matches!(rows.get(target_row), Some(InspectorRow::Field { .. })) {
        return Some(target_row);
    }
    for row in target_row..rows.len() {
        if matches!(rows.get(row), Some(InspectorRow::Field { .. })) {
            return Some(row);
        }
    }
    for row in (0..target_row).rev() {
        if matches!(rows.get(row), Some(InspectorRow::Field { .. })) {
            return Some(row);
        }
    }
    None
}
