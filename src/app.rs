use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;

use crate::action::Action;
use crate::cli::Cli;
use crate::clipboard;
use crate::commands::hints;
use crate::commands::parser::parse_command;
use crate::commands::types::Command;
use crate::config::Config;
use crate::copy::{format_selection, CopyDisplay, CopyFormat};
use crate::core::document::{ByteSlot, Document};
use crate::core::patch::PatchState;
use crate::error::{HxError, HxResult};
use crate::input::keymap::map_key;
use crate::input::mouse;
use crate::mode::{Mode, NibblePhase};
use crate::profile::{FrameStats, Profiler, RenderMainStats, StartupStats};
use crate::util::format::offset_width;
use crate::view::{ascii_grid, command_line, gutter, hex_grid, layout, palette::Palette, status};

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

#[derive(Debug, Clone, Copy)]
struct UndoEntry {
    offset: u64,
    previous_patch: PatchState,
    cursor_before: u64,
    mode_before: Mode,
}

#[derive(Debug, Clone)]
struct UndoStep {
    entries: Vec<UndoEntry>,
}

#[derive(Debug, Clone)]
struct SearchState {
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
enum SearchDirection {
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
        let cursor = if document.len() == 0 {
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

    fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        let profiling = self.profiler.is_some();
        let frame_start = profiling.then(Instant::now);
        let screen = layout::split_screen(frame.area(), self.mode == Mode::Command);
        self.last_command_area = screen.command;
        let main_start = profiling.then(Instant::now);
        let main_stats = self.render_main(frame, screen.main, profiling);
        let main_elapsed = main_start.map(|start| start.elapsed()).unwrap_or_default();
        let status_start = profiling.then(Instant::now);
        self.render_status(frame, screen.status);
        let status_elapsed = status_start
            .map(|start| start.elapsed())
            .unwrap_or_default();
        let command_start = profiling.then(Instant::now);
        if let Some(command_area) = screen.command {
            self.render_command(frame, command_area);
        }
        let command_elapsed = command_start
            .map(|start| start.elapsed())
            .unwrap_or_default();
        if let (Some(start), Some(profiler)) = (frame_start, self.profiler.as_mut()) {
            profiler.record_frame(
                FrameStats {
                    total: start.elapsed(),
                    main: main_elapsed,
                    status: status_elapsed,
                    command: command_elapsed,
                    main_stats,
                },
                self.document.io_stats(),
            );
        }
    }

    fn render_main(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        profiling: bool,
    ) -> RenderMainStats {
        let mut stats = RenderMainStats::default();
        let block = Block::default().borders(Borders::ALL);
        let columns = layout::split_main(&block, area, offset_width(self.document.len()) as u16);
        self.last_columns = Some(columns);
        frame.render_widget(block, area);

        // Keep render-derived row count in sync with navigation and paging.
        self.view_rows = columns.gutter.height.max(1) as usize;
        stats.rows = self.view_rows;

        let row_collect_start = profiling.then(Instant::now);
        let row_count = columns.gutter.height as usize;
        let mut row_offsets = Vec::with_capacity(row_count);
        let mut rows = Vec::with_capacity(row_count);
        for row in 0..row_count {
            let offset = self.viewport_top + row as u64 * self.config.bytes_per_line as u64;
            row_offsets.push(offset);
            let row_data = self
                .document
                .row_bytes(offset, self.config.bytes_per_line)
                .unwrap_or_else(|_| vec![ByteSlot::Empty; self.config.bytes_per_line]);
            rows.push(row_data);
        }
        if let Some(start) = row_collect_start {
            stats.row_collect = start.elapsed();
        }

        let line_build_start = profiling.then(Instant::now);
        let gutter_lines = if self.document.len() == 0 {
            vec![Line::raw("No data")]
        } else {
            gutter::build(
                &row_offsets,
                offset_width(self.document.len()),
                &self.palette,
            )
        };
        let hex_lines = if self.document.len() == 0 {
            vec![Line::raw("No content")]
        } else {
            hex_grid::build(
                &rows,
                &row_offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
                self.selection_range(),
            )
        };
        let ascii_lines = if self.document.len() == 0 {
            vec![Line::raw("")]
        } else {
            ascii_grid::build(
                &rows,
                &row_offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
                self.selection_range(),
            )
        };
        if let Some(start) = line_build_start {
            stats.line_build = start.elapsed();
        }

        let widget_draw_start = profiling.then(Instant::now);
        frame.render_widget(Paragraph::new(gutter_lines), columns.gutter);
        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            columns.sep1,
        );
        frame.render_widget(
            Paragraph::new(hex_lines).wrap(Wrap { trim: false }),
            columns.hex,
        );
        frame.render_widget(
            separator_widget(columns.gutter.height, &self.palette),
            columns.sep2,
        );
        frame.render_widget(Paragraph::new(ascii_lines), columns.ascii);
        if let Some(start) = widget_draw_start {
            stats.widget_draw = start.elapsed();
        }
        stats
    }

    fn render_status(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let path_display = self.document.path().to_string_lossy();
        let line = status::build(
            status::StatusInfo {
                mode: self.mode,
                path: &path_display,
                cursor: self.cursor,
                len: self.document.len(),
                selection_len: self.selection_range().map(|(start, end)| end - start + 1),
                paste_info: self.last_paste.as_ref().map(|state| state.summary.as_str()),
                dirty: self.document.is_dirty(),
                message: &self.status_message,
                readonly: self.document.is_readonly(),
            },
            &self.palette,
        );
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_command(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let hint = hints::hint_for(&self.command_buffer);
        let widget = command_line::widget(&self.command_buffer, hint, &self.palette);
        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        frame.render_widget(widget, area);
        frame.set_cursor_position((inner.x + 1 + self.command_buffer.len() as u16, inner.y));
    }

    fn handle_action(&mut self, action: Action) -> Result<()> {
        let result = match action.clone() {
            Action::MoveLeft => self.move_horizontal(-1),
            Action::MoveRight => self.move_horizontal(1),
            Action::MoveUp => self.move_vertical(-1),
            Action::MoveDown => self.move_vertical(1),
            Action::PageUp => self.move_vertical(-(self.view_rows as i64)),
            Action::PageDown => self.move_vertical(self.view_rows as i64),
            Action::RowStart => self.move_row_edge(false),
            Action::RowEnd => self.move_row_edge(true),
            Action::ToggleVisual => self.toggle_visual(),
            Action::EnterEdit => {
                if self.document.is_readonly() {
                    Err(HxError::ReadOnly)
                } else {
                    self.mode = Mode::EditHex {
                        phase: NibblePhase::High,
                    };
                    Ok(())
                }
            }
            Action::EnterCommand => {
                self.command_return_mode = Some(self.mode);
                self.mode = Mode::Command;
                self.command_buffer.clear();
                Ok(())
            }
            Action::LeaveMode => {
                self.leave_mode();
                Ok(())
            }
            Action::DeleteByte => self.delete_at_cursor_or_selection(),
            Action::SearchNext => self.repeat_search(SearchDirection::Forward),
            Action::SearchPrev => self.repeat_search(SearchDirection::Backward),
            Action::Undo(steps) => self.undo(steps, true),
            Action::EditHex(value) => self.edit_nibble(value),
            Action::CommandChar(c) => {
                self.command_buffer.push(c);
                Ok(())
            }
            Action::CommandBackspace => {
                self.command_buffer.pop();
                Ok(())
            }
            Action::CommandSubmit => self.submit_command(),
            Action::CommandCancel => {
                self.command_buffer.clear();
                self.mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
                Ok(())
            }
            Action::ForceQuit => {
                self.should_quit = true;
                Ok(())
            }
        };

        match result {
            Ok(()) => {
                self.ensure_cursor_visible();
                if !matches!(action, Action::CommandChar(_) | Action::CommandBackspace) {
                    self.clear_error_if_command_done();
                }
                Ok(())
            }
            Err(err) => {
                self.status_message = err.to_string();
                Ok(())
            }
        }
    }

    fn handle_mouse(&mut self, mouse_event: MouseEvent) -> Result<()> {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_viewport(-3);
                Ok(())
            }
            MouseEventKind::ScrollDown => {
                self.scroll_viewport(3);
                Ok(())
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(columns) = self.last_columns else {
                    return Ok(());
                };

                if let Some(hit) = mouse::hit_test(
                    columns,
                    mouse_event.column,
                    mouse_event.row,
                    self.viewport_top,
                    self.config.bytes_per_line,
                    self.document.len(),
                ) {
                    self.mouse_selection_anchor = Some(hit.offset);
                    self.cursor = hit.offset;
                    match self.mode {
                        Mode::EditHex { .. } => {
                            self.mode = Mode::EditHex {
                                phase: hit.phase.unwrap_or(NibblePhase::High),
                            };
                        }
                        Mode::Command => {
                            self.command_buffer.clear();
                            self.mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
                        }
                        Mode::Visual => {
                            self.selection_anchor = None;
                            self.mode = Mode::Normal;
                        }
                        Mode::Normal => {}
                    }
                    self.ensure_cursor_visible();
                    return Ok(());
                }

                if matches!(self.mode, Mode::Command)
                    && self
                        .last_command_area
                        .is_some_and(|rect| contains(rect, mouse_event.column, mouse_event.row))
                {
                    return Ok(());
                }

                Ok(())
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(columns) = self.last_columns else {
                    return Ok(());
                };
                let Some(hit) = mouse::hit_test(
                    columns,
                    mouse_event.column,
                    mouse_event.row,
                    self.viewport_top,
                    self.config.bytes_per_line,
                    self.document.len(),
                ) else {
                    return Ok(());
                };

                let anchor = self.mouse_selection_anchor.unwrap_or(hit.offset);
                self.selection_anchor = Some(anchor);
                self.cursor = hit.offset;
                self.mode = Mode::Visual;
                self.ensure_cursor_visible();
                Ok(())
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_selection_anchor = None;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn move_horizontal(&mut self, delta: i64) -> HxResult<()> {
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = NibblePhase::High;
        }
        Ok(())
    }

    fn move_vertical(&mut self, rows: i64) -> HxResult<()> {
        let delta = rows.saturating_mul(self.config.bytes_per_line as i64);
        self.cursor = self.offset_with_delta(self.cursor, delta);
        if let Mode::EditHex { ref mut phase } = self.mode {
            *phase = NibblePhase::High;
        }
        Ok(())
    }

    fn move_row_edge(&mut self, end: bool) -> HxResult<()> {
        let row_start = align_offset(self.cursor, self.config.bytes_per_line);
        let target = if end {
            row_start + self.config.bytes_per_line.saturating_sub(1) as u64
        } else {
            row_start
        };
        self.cursor = self.clamp_offset(target);
        Ok(())
    }

    fn delete_at_cursor_or_selection(&mut self) -> HxResult<()> {
        if matches!(self.mode, Mode::Visual) {
            return self.delete_selection();
        }
        self.delete_current()
    }

    fn delete_current(&mut self) -> HxResult<()> {
        let previous_patch = self.document.patch_state_at(self.cursor);
        self.document.delete_byte(self.cursor)?;
        self.push_undo_if_changed(self.cursor, previous_patch, self.cursor, self.mode);
        self.status_message = format!("deleted 0x{:x}", self.cursor);
        Ok(())
    }

    fn delete_selection(&mut self) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return self.delete_current();
        };

        let cursor_before = start;
        let mode_before = Mode::Visual;
        let mut undo_entries = Vec::with_capacity((end - start + 1) as usize);
        for offset in start..=end {
            let previous_patch = self.document.patch_state_at(offset);
            self.document.delete_byte(offset)?;
            if self.document.patch_state_at(offset) != previous_patch {
                undo_entries.push(UndoEntry {
                    offset,
                    previous_patch,
                    cursor_before,
                    mode_before,
                });
            }
        }

        self.push_undo_step(undo_entries);
        self.cursor = self.clamp_offset(start);
        self.selection_anchor = None;
        self.mode = Mode::Normal;
        self.status_message = format!("deleted selection {} bytes", end - start + 1);
        Ok(())
    }

    fn edit_nibble(&mut self, value: u8) -> HxResult<()> {
        let offset = self.cursor;
        let phase = match self.mode {
            Mode::EditHex { phase } => phase,
            _ => return Ok(()),
        };
        let previous_patch = self.document.patch_state_at(offset);
        self.document.replace_nibble(offset, phase, value)?;
        self.push_undo_if_changed(offset, previous_patch, self.cursor, self.mode);
        self.status_message = format!("edited 0x{:x}", offset);
        self.mode = match phase {
            NibblePhase::High => Mode::EditHex {
                phase: NibblePhase::Low,
            },
            NibblePhase::Low => {
                let next = offset + 1;
                self.cursor = next.min(self.document.len());
                Mode::EditHex {
                    phase: NibblePhase::High,
                }
            }
        };
        Ok(())
    }

    fn submit_command(&mut self) -> HxResult<()> {
        let return_mode = self.command_return_mode.unwrap_or(Mode::Normal);
        let command = parse_command(&self.command_buffer)?;
        self.command_buffer.clear();
        self.execute_command(command)?;
        self.mode = return_mode;
        self.command_return_mode = None;
        Ok(())
    }

    fn execute_command(&mut self, command: Command) -> HxResult<()> {
        match command {
            Command::Quit { force } => {
                if self.document.is_dirty() && !force {
                    return Err(HxError::DirtyQuit);
                }
                self.should_quit = true;
                Ok(())
            }
            Command::Write { path } => {
                let saved = self.document.save(path)?;
                self.undo_stack.clear();
                self.cursor = self.clamp_offset(self.cursor);
                self.status_message = format!("wrote {}", saved.display());
                Ok(())
            }
            Command::WriteQuit { path } => {
                let saved = self.document.save(path)?;
                self.undo_stack.clear();
                self.cursor = self.clamp_offset(self.cursor);
                self.status_message = format!("wrote {}", saved.display());
                self.should_quit = true;
                Ok(())
            }
            Command::Goto { offset } => {
                self.cursor = self.document.goto(offset)?;
                self.status_message = format!("goto 0x{:x}", self.cursor);
                Ok(())
            }
            Command::Undo { steps } => self.undo(steps, false),
            Command::Paste {
                raw,
                preview,
                limit,
            } => self.paste_from_clipboard(raw, preview, limit),
            Command::Copy { format, display } => self.copy_selection(format, display),
            Command::SearchAscii { pattern } => {
                let search = SearchState {
                    kind: SearchKind::Ascii,
                    pattern,
                };
                self.last_search = Some(search.clone());
                self.run_search(&search, SearchDirection::Forward)
            }
            Command::SearchHex { pattern } => {
                let search = SearchState {
                    kind: SearchKind::Hex,
                    pattern,
                };
                self.last_search = Some(search.clone());
                self.run_search(&search, SearchDirection::Forward)
            }
        }
    }

    fn undo(&mut self, steps: usize, restore_mode: bool) -> HxResult<()> {
        let mut undone = 0;

        for _ in 0..steps {
            let Some(step) = self.undo_stack.pop() else {
                break;
            };
            for entry in step.entries.iter().rev() {
                self.document
                    .restore_patch_state(entry.offset, entry.previous_patch)?;
            }
            if let Some(entry) = step.entries.first() {
                self.cursor = self.clamp_offset(entry.cursor_before);
                if restore_mode {
                    self.mode = entry.mode_before;
                }
            }
            undone += 1;
        }

        if !restore_mode {
            self.mode = Mode::Normal;
        }

        if undone == 0 {
            self.status_message = "nothing to undo".to_owned();
        } else if undone == 1 {
            self.status_message = "undid 1 action".to_owned();
        } else {
            self.status_message = format!("undid {undone} actions");
        }

        Ok(())
    }

    fn copy_selection(&mut self, format: CopyFormat, display: CopyDisplay) -> HxResult<()> {
        let Some((start, end)) = self.selection_range() else {
            return Err(HxError::MissingSelection);
        };
        let bytes = self.document.logical_bytes(start, end)?;
        let text = format_selection(&bytes, format, display)?;
        clipboard::copy_text(&text)?;
        self.status_message = format!(
            "copied {} bytes [{} {}]",
            bytes.len(),
            format.label(),
            display.label()
        );
        Ok(())
    }

    fn paste_from_clipboard(
        &mut self,
        raw: bool,
        preview: bool,
        limit: Option<usize>,
    ) -> HxResult<()> {
        let (mut bytes, source) = if raw {
            (clipboard::read_raw_bytes()?, PasteSource::Raw)
        } else {
            let text = clipboard::read_text()?;
            parse_paste_payload(&text)?
        };

        if let Some(limit) = limit {
            bytes.truncate(limit);
        }

        self.last_paste = Some(PasteState {
            summary: paste_summary(source, bytes.len(), preview, &bytes),
        });

        if preview {
            self.status_message = if bytes.is_empty() {
                "paste preview: no bytes".to_owned()
            } else {
                format!("paste preview [{} {} bytes]", source.label(), bytes.len())
            };
            return Ok(());
        }

        let pasted = self.apply_paste_bytes(&bytes)?;
        if pasted == 0 {
            self.status_message = "paste produced no bytes".to_owned();
        } else {
            self.status_message = format!("pasted {} bytes [{}]", pasted, source.label());
        }
        Ok(())
    }

    fn apply_paste_bytes(&mut self, bytes: &[u8]) -> HxResult<usize> {
        if self.document.is_readonly() {
            return Err(HxError::ReadOnly);
        }
        if bytes.is_empty() {
            return Ok(0);
        }

        let cursor_before = self.cursor;
        let mode_before = self.mode;
        let mut undo_entries = Vec::with_capacity(bytes.len());
        for (idx, &byte) in bytes.iter().enumerate() {
            let offset = cursor_before + idx as u64;
            let previous_patch = self.document.patch_state_at(offset);
            self.document.set_byte(offset, byte)?;
            if self.document.patch_state_at(offset) != previous_patch {
                undo_entries.push(UndoEntry {
                    offset,
                    previous_patch,
                    cursor_before,
                    mode_before,
                });
            }
        }

        self.push_undo_step(undo_entries);

        self.cursor = self.clamp_offset(cursor_before + bytes.len().saturating_sub(1) as u64);
        Ok(bytes.len())
    }

    fn push_undo_if_changed(
        &mut self,
        offset: u64,
        previous_patch: PatchState,
        cursor_before: u64,
        mode_before: Mode,
    ) {
        if self.document.patch_state_at(offset) != previous_patch {
            self.push_undo_step(vec![UndoEntry {
                offset,
                previous_patch,
                cursor_before,
                mode_before,
            }]);
        }
    }

    fn push_undo_step(&mut self, entries: Vec<UndoEntry>) {
        if entries.is_empty() {
            return;
        }
        self.undo_stack.push(UndoStep { entries });
    }

    fn repeat_search(&mut self, direction: SearchDirection) -> HxResult<()> {
        let Some(search) = self.last_search.clone() else {
            self.status_message = "no active search".to_owned();
            return Ok(());
        };
        self.run_search(&search, direction)
    }

    fn run_search(&mut self, search: &SearchState, direction: SearchDirection) -> HxResult<()> {
        let started_at = Instant::now();
        let found = match direction {
            SearchDirection::Forward => {
                let start = if self.document.len() == 0 {
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

    fn toggle_visual(&mut self) -> HxResult<()> {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Normal => {
                self.selection_anchor = Some(self.cursor);
                self.mode = Mode::Visual;
            }
            Mode::EditHex { .. } | Mode::Command => {}
        }
        Ok(())
    }

    fn ensure_cursor_visible(&mut self) {
        let row_size = self.config.bytes_per_line as u64;
        let cursor_row = align_offset(self.cursor, self.config.bytes_per_line);
        let visible_rows = self.visible_rows();
        let bottom = self.viewport_top + visible_rows.saturating_sub(1) * row_size;
        if cursor_row < self.viewport_top {
            self.viewport_top = cursor_row;
        } else if cursor_row > bottom {
            self.viewport_top =
                cursor_row.saturating_sub((visible_rows.saturating_sub(1)) * row_size);
        }
        self.viewport_top = align_offset(self.viewport_top, self.config.bytes_per_line);
    }

    fn scroll_viewport(&mut self, rows: i64) {
        if self.document.len() == 0 {
            return;
        }
        let max_top = self.max_viewport_top();
        let delta = rows.saturating_mul(self.config.bytes_per_line as i64);
        self.viewport_top = if delta >= 0 {
            self.viewport_top.saturating_add(delta as u64).min(max_top)
        } else {
            self.viewport_top.saturating_sub(delta.unsigned_abs())
        };
        self.viewport_top =
            align_offset(self.viewport_top, self.config.bytes_per_line).min(max_top);
        self.clamp_cursor_into_view();
    }

    fn clamp_cursor_into_view(&mut self) {
        if self.document.len() == 0 {
            self.cursor = 0;
            return;
        }
        let row_size = self.config.bytes_per_line as u64;
        let visible_rows = self.visible_rows();
        let visible_start = self.viewport_top;
        let visible_end = (self.viewport_top + visible_rows.saturating_mul(row_size))
            .min(self.document.len())
            .saturating_sub(1);
        self.cursor = self.cursor.clamp(visible_start, visible_end);
    }

    fn max_viewport_top(&self) -> u64 {
        if self.document.len() == 0 {
            return 0;
        }
        let row_size = self.config.bytes_per_line as u64;
        let visible_rows = self.visible_rows();
        let tail_rows = self.document.len().saturating_sub(1) / row_size;
        tail_rows
            .saturating_sub(visible_rows.saturating_sub(1))
            .saturating_mul(row_size)
    }

    fn visible_rows(&self) -> u64 {
        self.view_rows.max(1) as u64
    }

    fn offset_with_delta(&self, current: u64, delta: i64) -> u64 {
        if self.document.len() == 0 {
            return 0;
        }
        let max = self.document.len() - 1;
        if delta >= 0 {
            current.saturating_add(delta as u64).min(max)
        } else {
            current.saturating_sub(delta.unsigned_abs()).min(max)
        }
    }

    fn clamp_offset(&self, offset: u64) -> u64 {
        if self.document.len() == 0 {
            0
        } else {
            offset.min(self.document.len() - 1)
        }
    }

    fn clear_error_if_command_done(&mut self) {
        let is_error = self.status_message.starts_with("invalid")
            || self.status_message.starts_with("unknown")
            || self.status_message.starts_with("missing")
            || self.status_message.contains("outside");
        if !matches!(self.mode, Mode::Command) && is_error {
            self.status_message.clear();
        }
    }

    fn leave_mode(&mut self) {
        match self.mode {
            Mode::Visual => {
                self.selection_anchor = None;
                self.mode = Mode::Normal;
            }
            Mode::Command => {
                self.mode = self.command_return_mode.take().unwrap_or(Mode::Normal);
            }
            _ => {
                self.mode = Mode::Normal;
            }
        }
    }

    fn selection_range(&self) -> Option<(u64, u64)> {
        let anchor = self.selection_anchor?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }
}

/// Snap an offset to the first byte of its visual row.
fn align_offset(offset: u64, bytes_per_line: usize) -> u64 {
    if bytes_per_line == 0 {
        offset
    } else {
        offset / bytes_per_line as u64 * bytes_per_line as u64
    }
}

fn separator_widget(height: u16, palette: &Palette) -> Paragraph<'static> {
    let lines = (0..height)
        .map(|_| Line::styled("│", palette.separator))
        .collect::<Vec<_>>();
    Paragraph::new(lines)
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn parse_paste_payload(text: &str) -> HxResult<(Vec<u8>, PasteSource)> {
    if let Ok(hex) = crate::util::parse::parse_hex_stream(text) {
        return Ok((hex, PasteSource::Hex));
    }
    if let Ok(base64) = crate::util::parse::decode_base64(text) {
        return Ok((base64, PasteSource::Base64));
    }
    Err(HxError::InvalidPasteData(text.trim().to_owned()))
}

fn paste_summary(source: PasteSource, bytes: usize, preview: bool, data: &[u8]) -> String {
    let head = data
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let suffix = if data.len() > 4 { " …" } else { "" };
    if preview {
        format!("pv {}:{} [{}{}]", source.label(), bytes, head, suffix)
    } else {
        format!("ps {}:{} [{}{}]", source.label(), bytes, head, suffix)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn app_with_len(len: usize) -> App {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, vec![0_u8; len]).unwrap();
        let cli = Cli {
            file,
            bytes_per_line: 16,
            page_size: 4096,
            cache_pages: 8,
            profile: false,
            readonly: false,
            no_color: true,
            offset: None,
        };
        let mut app = App::from_cli(cli).unwrap();
        app.view_rows = 4;
        app
    }

    fn app_with_bytes(bytes: &[u8]) -> App {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, bytes).unwrap();
        let cli = Cli {
            file,
            bytes_per_line: 16,
            page_size: 4096,
            cache_pages: 8,
            profile: false,
            readonly: false,
            no_color: true,
            offset: None,
        };
        let mut app = App::from_cli(cli).unwrap();
        app.view_rows = 4;
        app
    }

    #[test]
    fn scroll_viewport_moves_top_down() {
        let mut app = app_with_len(256);
        app.scroll_viewport(3);
        assert_eq!(app.viewport_top, 48);
    }

    #[test]
    fn scroll_viewport_clamps_cursor_into_visible_range() {
        let mut app = app_with_len(256);
        app.cursor = 0;
        app.scroll_viewport(3);
        assert_eq!(app.cursor, 48);
    }

    #[test]
    fn scroll_viewport_stops_at_last_page() {
        let mut app = app_with_len(256);
        app.scroll_viewport(99);
        assert_eq!(app.viewport_top, 192);
    }

    #[test]
    fn edit_mode_undo_restores_previous_nibble_state() {
        let mut app = app_with_len(16);
        app.mode = Mode::EditHex {
            phase: NibblePhase::High,
        };

        app.edit_nibble(0xa).unwrap();
        assert_eq!(app.cursor, 0);
        assert_eq!(
            app.mode,
            Mode::EditHex {
                phase: NibblePhase::Low
            }
        );

        app.undo(1, true).unwrap();
        assert_eq!(app.cursor, 0);
        assert_eq!(
            app.mode,
            Mode::EditHex {
                phase: NibblePhase::High
            }
        );
        assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0));
    }

    #[test]
    fn command_undo_can_rewind_multiple_changes() {
        let mut app = app_with_len(16);
        app.mode = Mode::EditHex {
            phase: NibblePhase::High,
        };
        app.edit_nibble(0xa).unwrap();
        app.edit_nibble(0xb).unwrap();
        app.mode = Mode::Normal;

        app.execute_command(Command::Undo { steps: 2 }).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.cursor, 0);
        assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0));
    }

    #[test]
    fn toggling_visual_tracks_selection_range() {
        let mut app = app_with_len(32);
        app.toggle_visual().unwrap();
        assert_eq!(app.mode, Mode::Visual);
        assert_eq!(app.selection_range(), Some((0, 0)));

        app.move_horizontal(3).unwrap();
        assert_eq!(app.selection_range(), Some((0, 3)));

        app.toggle_visual().unwrap();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selection_range(), None);
    }

    #[test]
    fn visual_delete_removes_range_as_one_action() {
        let mut app = app_with_bytes(&[0x10, 0x11, 0x12, 0x13]);
        app.toggle_visual().unwrap();
        app.move_horizontal(2).unwrap();
        app.delete_at_cursor_or_selection().unwrap();

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.cursor, 0);
        assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Deleted);
        assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Deleted);
        assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Deleted);
        assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0x13));

        app.undo(1, true).unwrap();
        assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x10));
        assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x11));
        assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x12));
    }

    #[test]
    fn search_next_and_prev_follow_last_pattern() {
        let mut app = app_with_bytes(b"abc hello xyz hello end");
        app.execute_command(Command::SearchAscii {
            pattern: b"hello".to_vec(),
        })
        .unwrap();
        assert_eq!(app.cursor, 4);

        app.repeat_search(SearchDirection::Forward).unwrap();
        assert_eq!(app.cursor, 14);

        app.repeat_search(SearchDirection::Backward).unwrap();
        assert_eq!(app.cursor, 4);
    }

    #[test]
    fn paste_overwrites_and_appends_past_eof() {
        let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
        app.cursor = 1;
        assert_eq!(app.apply_paste_bytes(&[0xaa, 0xbb, 0xcc]).unwrap(), 3);
        assert_eq!(app.document.len(), 4);
        assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));
        assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xaa));
        assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0xbb));
        assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Present(0xcc));
    }

    #[test]
    fn undo_reverts_entire_paste_as_one_action() {
        let mut app = app_with_bytes(&[0x11, 0x22, 0x33]);
        app.cursor = 1;
        app.apply_paste_bytes(&[0xaa, 0xbb, 0xcc]).unwrap();
        app.undo(1, true).unwrap();

        assert_eq!(app.document.len(), 3);
        assert_eq!(app.document.byte_at(0).unwrap(), ByteSlot::Present(0x11));
        assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0x22));
        assert_eq!(app.document.byte_at(2).unwrap(), ByteSlot::Present(0x33));
        assert_eq!(app.document.byte_at(3).unwrap(), ByteSlot::Empty);
    }

    #[test]
    fn edit_mode_can_append_at_eof() {
        let mut app = app_with_bytes(&[0x11]);
        app.mode = Mode::EditHex {
            phase: NibblePhase::High,
        };
        app.cursor = 1;
        app.edit_nibble(0xa).unwrap();
        app.edit_nibble(0xb).unwrap();
        assert_eq!(app.document.len(), 2);
        assert_eq!(app.document.byte_at(1).unwrap(), ByteSlot::Present(0xab));
    }
}
