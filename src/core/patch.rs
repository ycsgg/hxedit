use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatchState {
    #[default]
    Unmodified,
    Replaced(u8),
    Deleted,
}

#[derive(Debug, Default, Clone)]
pub struct PatchSet {
    replacements: BTreeMap<u64, u8>,
    deletions: BTreeSet<u64>,
}

impl PatchSet {
    pub fn replacements(&self) -> &BTreeMap<u64, u8> {
        &self.replacements
    }

    pub fn deletions(&self) -> &BTreeSet<u64> {
        &self.deletions
    }

    pub fn replacement_at(&self, offset: u64) -> Option<u8> {
        self.replacements.get(&offset).copied()
    }

    pub fn state_at(&self, offset: u64) -> PatchState {
        if self.is_deleted(offset) {
            PatchState::Deleted
        } else if let Some(value) = self.replacement_at(offset) {
            PatchState::Replaced(value)
        } else {
            PatchState::Unmodified
        }
    }

    pub fn is_deleted(&self, offset: u64) -> bool {
        self.deletions.contains(&offset)
    }

    pub fn set_replacement(&mut self, offset: u64, value: u8) {
        self.apply_state(offset, PatchState::Replaced(value));
    }

    pub fn mark_deleted(&mut self, offset: u64) {
        self.apply_state(offset, PatchState::Deleted);
    }

    pub fn apply_state(&mut self, offset: u64, state: PatchState) {
        self.replacements.remove(&offset);
        self.deletions.remove(&offset);
        match state {
            PatchState::Unmodified => {}
            PatchState::Replaced(value) => {
                self.replacements.insert(offset, value);
            }
            PatchState::Deleted => {
                self.deletions.insert(offset);
            }
        }
    }

    pub fn clear(&mut self) {
        self.replacements.clear();
        self.deletions.clear();
    }

    pub fn has_deletions(&self) -> bool {
        !self.deletions.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        !self.replacements.is_empty() || !self.deletions.is_empty()
    }
}
