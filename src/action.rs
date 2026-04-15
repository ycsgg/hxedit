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
    LeaveMode,
    DeleteByte,
    SearchNext,
    SearchPrev,
    Undo(usize),
    EditHex(u8),
    EditBackspace,
    CommandChar(char),
    CommandBackspace,
    CommandSubmit,
    CommandCancel,
    ForceQuit,

    // ── Inspector actions ──
    /// Toggle inspector panel visibility / focus.
    ToggleInspector,
    /// Inspector mode: select previous field.
    InspectorUp,
    /// Inspector mode: select next field.
    InspectorDown,
    /// Inspector mode: begin editing / submit edit.
    InspectorEnter,
    /// Inspector editing: input character.
    InspectorChar(char),
    /// Inspector editing: backspace.
    InspectorBackspace,
    /// Inspector editing: move cursor left.
    InspectorLeft,
    /// Inspector editing: move cursor right.
    InspectorRight,
    /// Inspector editing: move cursor to buffer start.
    InspectorHome,
    /// Inspector editing: move cursor to buffer end.
    InspectorEnd,
    /// Inspector editing: delete character at cursor.
    InspectorDelete,
}
