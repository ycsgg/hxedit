use super::*;

impl Document {
    /// Overwrite a run of consecutive display cells starting at `offset`,
    /// generating each cell's new byte from its zero-based position in the run
    /// via `byte_at`. Streams cell resolution in 64 KB batches so a multi-GB
    /// fill never resolves every `CellId` up front.
    ///
    /// Matches the overwrite-paste contract used by `:fill`/`:zero`: writes are
    /// clamped to the current display length (bytes past EOF are dropped), and
    /// hitting a tombstoned cell is an error (overwrite does not skip slots).
    /// Returns the run length actually written plus the per-cell replacement
    /// changes for undo.
    pub fn overwrite_run_positional(
        &mut self,
        offset: u64,
        run_len: u64,
        mut byte_at: impl FnMut(u64) -> u8,
    ) -> HxResult<(u64, Vec<ReplacementDelta>)> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if run_len == 0 || offset >= doc_len {
            return Ok((0, Vec::new()));
        }
        let applied = run_len.min(doc_len - offset);
        let mut changes = Vec::new();
        let mut written = 0_u64;

        while written < applied {
            let batch = (applied - written).min(REPLACEMENT_CHUNK);
            let ids = self.cell_ids_range(offset + written, batch);
            for (i, id) in ids.into_iter().enumerate() {
                if self.is_tombstone(id) {
                    return Err(HxError::OffsetOutOfRange);
                }
                let value = byte_at(written + i as u64);
                let before = self.replacement_state(id)?;
                self.set_display_byte_by_id(id, value)?;
                let after = self.replacement_state(id)?;
                if after != before {
                    changes.push((id, before, after));
                }
            }
            written += batch;
        }

        Ok((applied, changes))
    }

    /// Overwrite a display run without retaining per-byte undo deltas.
    ///
    /// Returns the number of cells written and the number of replacement states
    /// that changed. Callers are responsible for recording a compact undo op
    /// when appropriate.
    pub fn overwrite_run_positional_compact(
        &mut self,
        offset: u64,
        run_len: u64,
        mut byte_at: impl FnMut(u64) -> u8,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if run_len == 0 || offset >= doc_len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let applied = run_len.min(doc_len - offset);
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };

        while stats.visited < applied {
            let batch = (applied - stats.visited).min(REPLACEMENT_CHUNK);
            let ids = self.cell_ids_range(offset + stats.visited, batch);
            for (i, id) in ids.into_iter().enumerate() {
                if self.is_tombstone(id) {
                    return Err(HxError::OffsetOutOfRange);
                }
                let value = byte_at(stats.visited + i as u64);
                let before = self.replacement_state(id)?;
                self.set_display_byte_by_id(id, value)?;
                let after = self.replacement_state(id)?;
                if after != before {
                    stats.changed += 1;
                }
            }
            stats.visited += batch;
        }

        Ok(stats)
    }

    /// Overwrite a display run with a repeating pattern using range overlays.
    ///
    /// This is the large clean-range fast path for `:fill` / `:zero`: the
    /// command's overwrite intent marks every covered cell changed, so the
    /// fast path can install one range per contiguous piece overlap without
    /// scanning base bytes to detect no-op positions.
    pub fn overwrite_run_pattern_overlay(
        &mut self,
        offset: u64,
        run_len: u64,
        pattern: &[u8],
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if pattern.is_empty() || run_len == 0 || offset >= doc_len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let applied = run_len.min(doc_len - offset);
        let end = offset + applied;
        let pattern: Arc<[u8]> = Arc::from(pattern);
        let pattern_len = pattern.len() as u64;
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        let mut segments = Vec::new();

        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= offset {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = offset.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let range_len = overlap_end - overlap_start;
                let phase = (overlap_start - offset) % pattern_len;
                stats.visited += range_len;
                stats.changed += range_len;
                segments.push((piece.source, source_start, range_len, phase));
            }
            display_cursor = piece_end;
        }

        if stats.visited > 0 {
            for (source, source_start, range_len, phase) in segments {
                self.replacements.set_pattern_range(
                    source,
                    source_start,
                    range_len,
                    Arc::clone(&pattern),
                    phase,
                );
            }
        }

        Ok(stats)
    }

    /// Overwrite a display run with explicit bytes using range overlays.
    ///
    /// This is the compact replacement primitive for clean overwrite-paste
    /// runs. The bytes are not repeated; each covered display cell takes the
    /// corresponding byte from `bytes`.
    pub fn overwrite_run_bytes_overlay(
        &mut self,
        offset: u64,
        bytes: Arc<[u8]>,
    ) -> HxResult<CompactReplacementStats> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if bytes.is_empty() || offset >= doc_len {
            return Ok(CompactReplacementStats {
                visited: 0,
                changed: 0,
            });
        }

        let applied = (bytes.len() as u64).min(doc_len - offset);
        let end = offset + applied;
        let mut stats = CompactReplacementStats {
            visited: 0,
            changed: 0,
        };
        let mut segments = Vec::new();

        let pieces = self.pieces_snapshot();
        let mut display_cursor = 0_u64;
        for piece in pieces {
            if display_cursor >= end {
                break;
            }
            let piece_end = display_cursor + piece.len;
            if piece_end <= offset {
                display_cursor = piece_end;
                continue;
            }

            let overlap_start = offset.max(display_cursor);
            let overlap_end = end.min(piece_end);
            if overlap_start < overlap_end {
                let source_start = piece.start + (overlap_start - display_cursor);
                let range_len = overlap_end - overlap_start;
                let phase = overlap_start - offset;
                stats.visited += range_len;
                stats.changed += range_len;
                segments.push((piece.source, source_start, range_len, phase));
            }
            display_cursor = piece_end;
        }

        if stats.visited > 0 {
            for (source, source_start, range_len, phase) in segments {
                self.replacements.set_bytes_range(
                    source,
                    source_start,
                    range_len,
                    Arc::clone(&bytes),
                    phase,
                );
            }
        }

        Ok(stats)
    }

    /// Apply bytes overlays only for cells whose current visible byte differs
    /// from the pasted byte. Returns the clamped write length plus compact
    /// `(display_offset, bytes)` runs suitable for undo/redo records.
    pub fn overwrite_run_bytes_overlay_changed(
        &mut self,
        offset: u64,
        bytes: &[u8],
    ) -> HxResult<(u64, Vec<BytesOverlayRun>)> {
        if self.readonly {
            return Err(HxError::ReadOnly);
        }
        let doc_len = self.len();
        if bytes.is_empty() || offset >= doc_len {
            return Ok((0, Vec::new()));
        }

        let applied = bytes.len().min((doc_len - offset) as usize);
        let end_inclusive = offset + applied as u64 - 1;
        let mut runs: Vec<BytesOverlayRun> = Vec::new();
        let mut run_start = 0_u64;
        let mut run_bytes = Vec::new();

        let mut expected_display = offset;
        self.walk_visible_byte_segments(
            offset,
            end_inclusive,
            REPLACEMENT_CHUNK as usize,
            |segment| {
                if segment.display_start != expected_display {
                    return Err(HxError::OffsetOutOfRange);
                }

                let start_index = (segment.display_start - offset) as usize;
                let end_index = start_index + segment.bytes.len();
                let target = &bytes[start_index..end_index];
                for (idx, (&current, &value)) in segment.bytes.iter().zip(target.iter()).enumerate()
                {
                    let display_offset = segment.display_start + idx as u64;
                    if current != value {
                        if run_bytes.is_empty() {
                            run_start = display_offset;
                        } else if run_start + run_bytes.len() as u64 != display_offset {
                            let bytes: Arc<[u8]> = Arc::from(std::mem::take(&mut run_bytes));
                            runs.push((run_start, bytes));
                            run_start = display_offset;
                        }
                        run_bytes.push(value);
                    } else if !run_bytes.is_empty() {
                        let bytes: Arc<[u8]> = Arc::from(std::mem::take(&mut run_bytes));
                        runs.push((run_start, bytes));
                    }
                }
                expected_display = segment.display_start + segment.bytes.len() as u64;
                Ok(WalkControl::Continue)
            },
        )?;
        if expected_display != offset + applied as u64 {
            return Err(HxError::OffsetOutOfRange);
        }

        if !run_bytes.is_empty() {
            let bytes: Arc<[u8]> = Arc::from(run_bytes);
            runs.push((run_start, bytes));
        }

        for (run_offset, run_bytes) in &runs {
            self.overwrite_run_bytes_overlay(*run_offset, Arc::clone(run_bytes))?;
        }

        Ok((applied as u64, runs))
    }
}
