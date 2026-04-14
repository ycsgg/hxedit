use std::time::Instant;

use crate::app::{App, SearchDirection, SearchState};
use crate::error::HxResult;

impl App {
    pub(crate) fn repeat_search(&mut self, direction: SearchDirection) -> HxResult<()> {
        let Some(search) = self.last_search.clone() else {
            self.status_message = "no active search".to_owned();
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
        let found = match direction {
            SearchDirection::Forward => {
                let start = if self.document.is_empty() {
                    0
                } else {
                    (self.cursor + 1).min(self.document.len())
                };
                self.document.search_forward(start, &search.pattern)?
            }
            SearchDirection::Backward => self
                .document
                .search_backward(self.cursor, &search.pattern)?,
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
            self.status_message = format!("found {} at 0x{:x}", search.kind.label(), found);
        } else {
            self.status_message = format!("{} pattern not found", search.kind.label());
        }

        Ok(())
    }
}
