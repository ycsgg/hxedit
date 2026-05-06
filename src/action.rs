#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    RowStart,
    RowEnd,
    EnterInsert,
    EnterReplace,
    ToggleVisual,
    EnterCommand,
    BeginDisasmEdit,
    LeaveMode,
    DeleteByte,
    SearchNext,
    SearchPrev,
    Undo(usize),
    EditHex(u8),
    EditBackspace,
    CommandChar(char),
    CommandLeft,
    CommandRight,
    CommandHome,
    CommandEnd,
    CommandDelete,
    CommandBackspace,
    CommandHistoryPrev,
    CommandHistoryNext,
    CommandSubmit,
    CommandCancel,
    ForceQuit,
    Redo(usize),

    // ── Side panel actions ──
    /// Toggle side panel visibility / focus.
    ToggleSidePanel,
    /// Side panel mode: select previous row/item.
    SidePanelUp,
    /// Side panel mode: select next row/item.
    SidePanelDown,
    /// Side panel mode: activate the selected row/item or submit an edit.
    SidePanelEnter,
    /// Side panel editing: input character.
    SidePanelChar(char),
    /// Side panel editing: backspace.
    SidePanelBackspace,
    /// Side panel editing: move cursor left.
    SidePanelLeft,
    /// Side panel editing: move cursor right.
    SidePanelRight,
    /// Side panel editing: move cursor to buffer start.
    SidePanelHome,
    /// Side panel editing: move cursor to buffer end.
    SidePanelEnd,
    /// Side panel editing: delete character at cursor.
    SidePanelDelete,
    /// Inspector page: toggle collapse/expand of current struct header.
    SidePanelToggleCollapse,
}
