use std::time::Instant;

use crate::app::{App, SearchDirection, SearchState};
use crate::error::HxResult;

impl App {
    pub(crate) fn repeat_search(&mut self, direction: SearchDirection) -> HxResult<()> {
        let Some(search) = self.last_search.clone() else {
            self.set_info_status("no active search");
            return Ok(());
        };
        self.run_search(&search, direction)
    }

    pub(crate) fn run_search(
        &mut self,
        search: &SearchState,
        direction: SearchDirection,
    ) -> HxResult<()> {
        let started_at = Instant::now();
        let (found, wrapped) = match direction {
            SearchDirection::Forward => {
                let start = if self.document.is_empty() {
                    0
                } else {
                    (self.cursor + 1).min(self.document.len())
                };
                if let Some(found) = self.document.search_forward(start, &search.pattern)? {
                    (Some(found), false)
                } else {
                    (self.document.search_forward(0, &search.pattern)?, start > 0)
                }
            }
            SearchDirection::Backward => {
                if let Some(found) = self
                    .document
                    .search_backward(self.cursor, &search.pattern)?
                {
                    (Some(found), false)
                } else {
                    (
                        self.document
                            .search_backward(self.document.len(), &search.pattern)?,
                        self.cursor < self.document.len(),
                    )
                }
            }
        };

        if let Some(profiler) = self.profiler.as_mut() {
            profiler.record_search(
                search.kind.label(),
                direction.label(),
                search.pattern.len(),
                started_at.elapsed(),
                found,
                self.document.io_stats(),
            );
        }

        if let Some(found) = found {
            self.cursor = found;
            if wrapped {
                self.set_info_status(format!(
                    "found {} at display 0x{:x} (wrapped)",
                    search.kind.label(),
                    found
                ));
            } else {
                self.set_info_status(format!(
                    "found {} at display 0x{:x}",
                    search.kind.label(),
                    found
                ));
            }
        } else {
            self.set_info_status(format!("{} pattern not found", search.kind.label()));
        }

        Ok(())
    }
}
