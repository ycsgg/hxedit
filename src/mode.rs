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
    EditHex {
        phase: NibblePhase,
    },
    InsertHex {
        pending: Option<PendingInsert>,
    },
    Visual,
    Command,
    /// Side panel has focus. Arrow keys navigate the active panel page.
    SidePanel,
    /// Inspector field is being edited inline.
    InspectorEdit,
    /// Disassembly instruction text is being edited inline.
    DisasmEdit,
}

impl Mode {
    pub fn is_side_panel(self) -> bool {
        matches!(self, Self::SidePanel | Self::InspectorEdit)
    }

    pub fn is_normal(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::EditHex { .. } => "EDIT",
            Self::InsertHex { .. } => "INSERT",
            Self::Visual => "VISUAL",
            Self::Command => "COMMAND",
            Self::SidePanel => "PANEL",
            Self::InspectorEdit => "INSPEDIT",
            Self::DisasmEdit => "ASMEDIT",
        }
    }
}
