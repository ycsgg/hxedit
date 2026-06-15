use super::*;

impl Document {
    /// Apply an in-place transform to every visible byte in a display range,
    /// streaming through the piece list in 64 KB chunks so a multi-GB
    /// selection never materializes its bytes twice in memory.
    ///
    /// Tombstoned cells are skipped (they carry no logical byte), matching the
    /// `logical_bytes` view. Each visited cell's current display byte (base
    /// byte with any replacement applied) is passed to `transform`; the result
    /// is written back as a replacement. Returns one
    /// `(cell, before_replacement, after_replacement)` entry per cell whose
    /// replacement state actually changed, plus the total count of visible
    /// (non-tombstone) cells visited, ready for the caller's undo record and
    /// status reporting.
    ///
    /// Pure replacement semantics: never inserts, tombstones, or real-deletes.
    pub fn transform_visible_range_in_place(
        &mut self,
        start: u64,
        end_inclusive: u64,
        mut transform: impl FnMut(u8) -> u8,
    ) -> HxResult<(u64, Vec<ReplacementDelta>)> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok((0, Vec::new()));
        }

        let mut changes = Vec::new();
        let mut visited = 0_u64;
        self.walk_visible_cells(start, end_inclusive, 64 * 1024, |document, chunk| {
            for cell in chunk.cells {
                if cell.deleted {
                    continue;
                }
                visited += 1;
                let updated = transform(cell.byte);
                let before = document.replacement_state(cell.id)?;
                document.set_display_byte_by_id(cell.id, updated)?;
                let after = document.replacement_state(cell.id)?;
                if after != before {
                    changes.push((cell.id, before, after));
                }
            }
            Ok(WalkControl::Continue)
        })?;

        Ok((visited, changes))
    }

    /// Streaming in-place transform without retaining per-byte undo deltas.
    ///
    /// Callers should only pair this with a compact undo record when the
    /// pre-edit range was checked with [`replacement_range_is_pristine`].
    pub fn transform_visible_range_in_place_compact(
        &mut self,
        start: u64,
        end_inclusive: u64,
        mut transform: impl FnMut(u8) -> u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        self.walk_visible_cells(
            start,
            end_inclusive,
            REPLACEMENT_CHUNK as usize,
            |document, chunk| {
                for cell in chunk.cells {
                    if cell.deleted {
                        continue;
                    }
                    stats.visited += 1;
                    let updated = transform(cell.byte);
                    let before = document.replacement_state(cell.id)?;
                    document.set_display_byte_by_id(cell.id, updated)?;
                    let after = document.replacement_state(cell.id)?;
                    if after != before {
                        stats.changed += 1;
                    }
                }
                Ok(WalkControl::Continue)
            },
        )?;

        Ok(stats)
    }

    /// Apply an XOR overlay over a clean display range without expanding one
    /// replacement entry per byte. Callers must only use this when the range
    /// has no existing tombstones/replacements.
    pub fn xor_visible_range_overlay(
        &mut self,
        start: u64,
        end_inclusive: u64,
        key: u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if key == 0 || len == 0 || start > end_inclusive || start >= len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }
        let end = end_inclusive.min(len - 1) + 1;
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };

        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let range_len = overlap_end - overlap_start;
                self.replacements
                    .set_xor_range(piece.source, source_start, range_len, key);
                stats.visited += range_len;
                stats.changed += range_len;
            }
            display_cursor = piece_end;
        }

        Ok(stats)
    }

    pub fn xor_visible_range_bytes_overlay_changed(
        &mut self,
        start: u64,
        end_inclusive: u64,
        key: u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        self.walk_visible_cells(
            start,
            end_inclusive,
            REPLACEMENT_CHUNK as usize,
            |document, chunk| {
                let mut run_start = 0_u64;
                let mut run_bytes = Vec::new();

                for cell in chunk.cells {
                    if cell.deleted {
                        document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                        continue;
                    }

                    stats.visited += 1;
                    let updated = cell.byte ^ key;
                    if updated == cell.byte {
                        document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                        continue;
                    }

                    stats.changed += 1;
                    if run_bytes.is_empty() {
                        run_start = cell.display_offset;
                    } else if run_start + run_bytes.len() as u64 != cell.display_offset {
                        document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                        run_start = cell.display_offset;
                    }
                    run_bytes.push(updated);
                }

                document.flush_bytes_overlay_run(run_start, &mut run_bytes)?;
                Ok(WalkControl::Continue)
            },
        )?;

        Ok(stats)
    }

    pub fn xor_visible_range_mixed_overlay(
        &mut self,
        start: u64,
        end_inclusive: u64,
        key: u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let len = self.len();
        if len == 0 || start > end_inclusive || start >= len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let end = end_inclusive.min(len - 1) + 1;
        let range_len = end - start;
        if self.display_range_has_tombstone(start, range_len)
            || self.display_range_has_sparse_replacement(start, range_len)
        {
            if !self.display_range_has_replacement_range(start, range_len) {
                return self.xor_visible_range_sparse_overlay(start, end - 1, key);
            }
            return self.xor_visible_range_bytes_overlay_changed(start, end - 1, key);
        }

        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let segment_len = overlap_end - overlap_start;
                self.replacements.xor_source_range_composed(
                    piece.source,
                    source_start,
                    segment_len,
                    key,
                );
                stats.visited += segment_len;
                if key != 0 {
                    stats.changed += segment_len;
                }
            }
            display_cursor = piece_end;
        }

        Ok(stats)
    }

    fn xor_visible_range_sparse_overlay(
        &mut self,
        start: u64,
        end_inclusive: u64,
        key: u8,
    ) -> HxResult<CompactReplacementStats> {
        let len = self.len();
        if key == 0 || len == 0 || start > end_inclusive || start >= len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let end = end_inclusive.min(len - 1) + 1;
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;

        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= start {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = start.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start >= overlap_end {
                display_cursor = piece_end;
                continue;
            }

            let source_start = piece.start + (overlap_start - display_cursor);
            let segment_len = overlap_end - overlap_start;
            let segment_end = source_start + segment_len;
            let tombstones =
                self.tombstones_in_source_range(piece.source, source_start, segment_len);
            let replacements =
                self.sparse_replacements_in_source_range(piece.source, source_start, segment_len);
            let mut tombstone_idx = 0usize;
            let mut replacement_idx = 0usize;
            let mut cursor = source_start;

            while tombstone_idx < tombstones.len() || replacement_idx < replacements.len() {
                let next_tombstone = tombstones.get(tombstone_idx).copied();
                let next_replacement = replacements.get(replacement_idx).map(|(offset, _)| *offset);
                let Some(next_dirty) = min_u64_option(next_tombstone, next_replacement) else {
                    break;
                };

                if cursor < next_dirty {
                    let span_len = next_dirty - cursor;
                    self.replacements.xor_source_range_composed(
                        piece.source,
                        cursor,
                        span_len,
                        key,
                    );
                    stats.visited += span_len;
                    stats.changed += span_len;
                }

                let mut is_tombstone = false;
                while tombstones
                    .get(tombstone_idx)
                    .is_some_and(|offset| *offset == next_dirty)
                {
                    is_tombstone = true;
                    tombstone_idx += 1;
                }

                let mut replacement = None;
                while replacements
                    .get(replacement_idx)
                    .is_some_and(|(offset, _)| *offset == next_dirty)
                {
                    replacement = replacements.get(replacement_idx).map(|(_, value)| *value);
                    replacement_idx += 1;
                }

                if !is_tombstone {
                    if let Some(value) = replacement {
                        let id = CellId::from_source(piece.source, next_dirty);
                        self.set_display_byte_by_id(id, value ^ key)?;
                        stats.visited += 1;
                        stats.changed += 1;
                    }
                }
                cursor = next_dirty.saturating_add(1);
            }

            if cursor < segment_end {
                let span_len = segment_end - cursor;
                self.replacements
                    .xor_source_range_composed(piece.source, cursor, span_len, key);
                stats.visited += span_len;
                stats.changed += span_len;
            }

            display_cursor = piece_end;
        }

        Ok(stats)
    }

    fn flush_bytes_overlay_run(&mut self, run_start: u64, run_bytes: &mut Vec<u8>) -> HxResult<()> {
        if run_bytes.is_empty() {
            return Ok(());
        }
        let bytes: Arc<[u8]> = Arc::from(std::mem::take(run_bytes));
        self.overwrite_run_bytes_overlay(run_start, bytes)?;
        Ok(())
    }
}

fn min_u64_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
