use std::collections::BTreeSet;
use std::io;
use std::time::{Duration, Instant};

mod clipboard_ops;
mod command_input;
mod commands;
mod editing_state;
mod events;
mod inspector_state;
mod mode_state;
mod mouse;
mod navigation;
mod render;
mod search;
#[cfg(test)]
mod tests;
mod text_cursor;
mod undo;

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::cli::Cli;
use crate::config::Config;
use crate::core::document::Document;
use crate::core::piece_table::CellId;
use crate::format::parse::{InspectorRow, NodePath};
use crate::input::keymap::map_key;
use crate::mode::Mode;
use crate::profile::{Profiler, StartupStats};
use crate::view::{layout, palette::Palette};
use navigation::align_offset;

/// Owns the editor runtime: document state, viewport state, and the TUI loop.
pub struct App {
    config: Config,
    document: Document,
    palette: Palette,
    mode: Mode,
    cursor: u64,
    viewport_top: u64,
    command_buffer: String,
    command_cursor_pos: usize,
    command_history: Vec<String>,
    command_history_index: Option<usize>,
    command_history_stash: Option<String>,
    status_message: String,
    status_level: StatusLevel,
    should_quit: bool,
    view_rows: usize,
    last_columns: Option<layout::MainColumns>,
    last_command_area: Option<Rect>,
    selection_anchor: Option<u64>,
    mouse_selection_anchor: Option<u64>,
    command_return_mode: Option<Mode>,
    undo_stack: Vec<UndoStep>,
    redo_stack: Vec<UndoStep>,
    last_search: Option<SearchState>,
    last_paste: Option<PasteState>,
    profiler: Option<Profiler>,
    // ── Inspector state ──
    /// Whether the inspector panel is shown.
    show_inspector: bool,
    /// Manual format override for the inspector, e.g. `elf`.
    inspector_format_override: Option<String>,
    /// Per-format entry cap for pagination-aware parsers (PNG / ZIP).
    /// Starts at `DEFAULT_ENTRY_CAP` and grows by `ENTRY_CAP_BATCH` on each
    /// `:insp more` until all entries are loaded.
    inspector_entry_cap: usize,
    /// Inspector runtime state. Some when show_inspector == true and format detected.
    inspector: Option<InspectorState>,
    /// Distinguishes “no detected format” from “detected but failed to parse”.
    inspector_error: Option<String>,
    /// Last non-fatal render read error already surfaced to stderr.
    last_render_error: Option<String>,
}

/// Inspector panel runtime state.
#[derive(Debug)]
pub(crate) struct InspectorState {
    /// Detected format name.
    pub format_name: String,
    /// Parsed structure value tree.
    pub structs: Vec<crate::format::parse::StructValue>,
    /// Flattened render rows (cached, rebuilt after edits).
    pub rows: Vec<InspectorRow>,
    /// Vertical scroll offset within the panel.
    pub scroll_offset: usize,
    /// Currently selected row index (index into `rows`).
    pub selected_row: usize,
    /// If editing a field, the edit state.
    pub editing: Option<InspectorEdit>,
    /// Paths of struct nodes whose children are hidden. Uses stable
    /// `(name, sibling_index)` paths so collapse state survives sibling
    /// insertions/removals between reparses.
    pub collapsed_nodes: BTreeSet<NodePath>,
}

/// Edit state for a field in the inspector panel.
#[derive(Debug, Clone)]
pub(crate) struct InspectorEdit {
    /// Index of the InspectorRow being edited.
    pub row_index: usize,
    /// Text edit buffer.
    pub buffer: String,
    /// Cursor position within the buffer.
    pub cursor_pos: usize,
}

/// Snapshot of a single cell's replacement state before an edit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplacementChange {
    pub(crate) id: CellId,
    /// `None` means the cell had no replacement (base byte was displayed).
    pub(crate) before: Option<u8>,
    /// `None` means the cell returns to its base byte.
    pub(crate) after: Option<u8>,
}

/// A single reversible edit operation.
///
/// Each variant stores enough information to undo itself:
/// - `Insert` — undo by real-deleting `len` bytes at `offset`.
/// - `RealDelete` — undo by re-inserting the saved `cells` at `offset`.
/// - `TombstoneDelete` — undo by clearing the tombstones for `ids`.
/// - `ReplaceBytes` — undo by restoring each cell's previous replacement.
#[derive(Debug, Clone)]
pub(crate) enum EditOp {
    Insert { offset: u64, cells: Vec<CellId> },
    RealDelete { offset: u64, cells: Vec<CellId> },
    TombstoneDelete { ids: Vec<CellId> },
    ReplaceBytes { changes: Vec<ReplacementChange> },
}

/// One entry on the undo stack: the cursor/mode before the edit, plus the
/// list of operations that were performed (replayed in reverse to undo).
#[derive(Debug, Clone)]
struct UndoStep {
    cursor_before: u64,
    mode_before: Mode,
    cursor_after: u64,
    mode_after: Mode,
    ops: Vec<EditOp>,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchState {
    kind: SearchKind,
    pattern: Vec<u8>,
}

#[derive(Debug, Clone)]
struct PasteState {
    summary: String,
}

#[derive(Debug, Clone, Copy)]
enum SearchKind {
    Ascii,
    Hex,
}

#[derive(Debug, Clone, Copy)]
enum PasteSource {
    Hex,
    Base64,
    Raw,
}

impl From<crate::util::parse::PasteTextSource> for PasteSource {
    fn from(source: crate::util::parse::PasteTextSource) -> Self {
        match source {
            crate::util::parse::PasteTextSource::Hex => Self::Hex,
            crate::util::parse::PasteTextSource::Base64 => Self::Base64,
        }
    }
}

impl SearchKind {
    fn label(self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::Hex => "hex",
        }
    }
}

impl PasteSource {
    fn label(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::Base64 => "base64",
            Self::Raw => "raw",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusLevel {
    Info,
    Notice,
    Warning,
    Error,
}

impl SearchDirection {
    fn label(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Backward => "backward",
        }
    }
}

impl App {
    pub(crate) fn clear_status(&mut self) {
        self.status_message.clear();
        self.status_level = StatusLevel::Info;
    }

    pub(crate) fn set_status(&mut self, level: StatusLevel, message: impl Into<String>) {
        self.status_message = message.into();
        self.status_level = level;
    }

    pub(crate) fn set_info_status(&mut self, message: impl Into<String>) {
        self.set_status(StatusLevel::Info, message);
    }

    pub(crate) fn set_notice_status(&mut self, message: impl Into<String>) {
        self.set_status(StatusLevel::Notice, message);
    }

    pub(crate) fn set_warning_status(&mut self, message: impl Into<String>) {
        self.set_status(StatusLevel::Warning, message);
    }

    pub(crate) fn set_error_status(&mut self, message: impl Into<String>) {
        self.set_status(StatusLevel::Error, message);
    }

    pub fn from_cli(cli: Cli) -> Result<Self> {
        let startup_begin = Instant::now();
        let config = cli.config()?;
        let after_config = Instant::now();
        let document = Document::open(&cli.file, &config)?;
        let after_open = Instant::now();
        let cursor = if document.is_empty() {
            0
        } else {
            config.initial_offset.min(document.len() - 1)
        };
        let after_init = Instant::now();

        let show_inspector = config.inspector;
        let mut app = Self {
            palette: Palette::new(config.color_level),
            viewport_top: align_offset(cursor, config.bytes_per_line),
            mode: Mode::Normal,
            command_buffer: String::new(),
            command_cursor_pos: 0,
            command_history: Vec::new(),
            command_history_index: None,
            command_history_stash: None,
            status_message: if document.is_readonly() && !config.readonly {
                "opened read-only; no write permission".to_owned()
            } else {
                String::new()
            },
            status_level: if document.is_readonly() && !config.readonly {
                StatusLevel::Warning
            } else {
                StatusLevel::Info
            },
            should_quit: false,
            view_rows: 1,
            last_columns: None,
            last_command_area: None,
            selection_anchor: None,
            mouse_selection_anchor: None,
            command_return_mode: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_search: None,
            last_paste: None,
            profiler: config.profile.then(|| {
                Profiler::new(StartupStats {
                    config_parse: after_config.duration_since(startup_begin),
                    document_open: after_open.duration_since(after_config),
                    app_init: after_init.duration_since(after_open),
                    terminal_setup: Duration::default(),
                })
            }),
            document,
            cursor,
            config,
            show_inspector,
            inspector_format_override: None,
            inspector_entry_cap: crate::format::detect::DEFAULT_ENTRY_CAP,
            inspector: None,
            inspector_error: None,
            last_render_error: None,
        };

        if app.show_inspector {
            app.refresh_inspector();
        }
        app.sync_inspector_to_cursor();
        Ok(app)
    }

    pub fn run(&mut self) -> Result<()> {
        let terminal_start = self.profiler.as_ref().map(|_| Instant::now());
        let session_start = self.profiler.as_ref().map(|_| Instant::now());
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        if let (Some(start), Some(profiler)) = (terminal_start, self.profiler.as_mut()) {
            profiler.set_terminal_setup(start.elapsed());
            profiler.log_startup(self.document.io_stats());
        }

        let result = self.run_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if let (Some(start), Some(profiler)) = (session_start, self.profiler.as_mut()) {
            profiler.set_session_wall(start.elapsed());
        }
        if let Some(profiler) = self.profiler.as_ref() {
            profiler.print_report(self.document.io_stats());
        }

        result
    }

    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            let poll_start = self.profiler.as_ref().map(|_| Instant::now());
            let has_event = event::poll(Duration::from_millis(250))?;
            if let (Some(start), Some(profiler)) = (poll_start, self.profiler.as_mut()) {
                profiler.record_poll(start.elapsed(), has_event);
            }
            if has_event {
                match event::read()? {
                    Event::Key(key) => {
                        if let Some(profiler) = self.profiler.as_mut() {
                            profiler.record_key_event();
                        }
                        if let Some(action) = map_key(self.mode, key) {
                            self.handle_action(action);
                        }
                    }
                    Event::Mouse(mouse) => {
                        if let Some(profiler) = self.profiler.as_mut() {
                            profiler.record_mouse_event();
                        }
                        self.handle_mouse(mouse)
                    }
                    _ => {
                        if let Some(profiler) = self.profiler.as_mut() {
                            profiler.record_other_event();
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
