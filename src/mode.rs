#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NibblePhase {
    High,
    Low,
}

impl NibblePhase {
    pub fn toggle(self) -> Self {
        match self {
            Self::High => Self::Low,
            Self::Low => Self::High,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingInsert {
    pub offset: u64,
    pub high_nibble: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    EditHex { phase: NibblePhase },
    InsertHex { pending: Option<PendingInsert> },
    Visual,
    Command,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::EditHex { .. } => "EDIT",
            Self::InsertHex { .. } => "INSERT",
            Self::Visual => "VISUAL",
            Self::Command => "COMMAND",
        }
    }
}
