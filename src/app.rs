use std::io;
use std::time::{Duration, Instant};

mod clipboard_ops;
mod commands;
mod events;
mod helpers;
mod navigation;
mod render;
mod search;
mod state;
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
    status_message: String,
    should_quit: bool,
    view_rows: usize,
    last_columns: Option<layout::MainColumns>,
    last_command_area: Option<Rect>,
    selection_anchor: Option<u64>,
    mouse_selection_anchor: Option<u64>,
    command_return_mode: Option<Mode>,
    undo_stack: Vec<UndoStep>,
    last_search: Option<SearchState>,
    last_paste: Option<PasteState>,
    profiler: Option<Profiler>,
}

/// Snapshot of a single cell's replacement state before an edit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplacementUndo {
    id: CellId,
    /// `None` means the cell had no replacement (base byte was displayed).
    previous: Option<u8>,
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
    Insert { offset: u64, len: u64 },
    RealDelete { offset: u64, cells: Vec<CellId> },
    TombstoneDelete { ids: Vec<CellId> },
    ReplaceBytes { changes: Vec<ReplacementUndo> },
}

/// One entry on the undo stack: the cursor/mode before the edit, plus the
/// list of operations that were performed (replayed in reverse to undo).
#[derive(Debug, Clone)]
struct UndoStep {
    cursor_before: u64,
    mode_before: Mode,
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

impl SearchDirection {
    fn label(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Backward => "backward",
        }
    }
}

impl App {
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
        Ok(Self {
            palette: Palette::new(config.color),
            viewport_top: align_offset(cursor, config.bytes_per_line),
            mode: Mode::Normal,
            command_buffer: String::new(),
            status_message: String::new(),
            should_quit: false,
            view_rows: 1,
            last_columns: None,
            last_command_area: None,
            selection_anchor: None,
            mouse_selection_anchor: None,
            command_return_mode: None,
            undo_stack: Vec::new(),
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
        })
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
                            self.handle_action(action)?;
                        }
                    }
                    Event::Mouse(mouse) => {
                        if let Some(profiler) = self.profiler.as_mut() {
                            profiler.record_mouse_event();
                        }
                        self.handle_mouse(mouse)?
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
