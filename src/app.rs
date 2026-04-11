use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};
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
use crate::commands::parser::parse_command;
use crate::commands::types::Command;
use crate::config::Config;
use crate::core::document::{ByteSlot, Document};
use crate::error::{HxError, HxResult};
use crate::input::keymap::map_key;
use crate::mode::{Mode, NibblePhase};
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
}

impl App {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        let config = cli.config()?;
        let document = Document::open(&cli.file, &config)?;
        let cursor = if document.original_len() == 0 {
            0
        } else {
            config.initial_offset.min(document.original_len() - 1)
        };
        Ok(Self {
            palette: Palette::new(config.color),
            viewport_top: align_offset(cursor, config.bytes_per_line),
            mode: Mode::Normal,
            command_buffer: String::new(),
            status_message: String::new(),
            should_quit: false,
            view_rows: 1,
            document,
            cursor,
            config,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if let Some(action) = map_key(self.mode, key) {
                        self.handle_action(action)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        let screen = layout::split_screen(frame.area(), self.mode == Mode::Command);
        self.render_main(frame, screen.main);
        self.render_status(frame, screen.status);
        if let Some(command_area) = screen.command {
            self.render_command(frame, command_area);
        }
    }

    fn render_main(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let block = Block::default().borders(Borders::ALL);
        let columns = layout::split_main(
            &block,
            area,
            offset_width(self.document.original_len()) as u16,
        );
        frame.render_widget(block, area);

        // Keep render-derived row count in sync with navigation and paging.
        self.view_rows = columns.gutter.height.max(1) as usize;

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

        let gutter_lines = if self.document.original_len() == 0 {
            vec![Line::raw("No data")]
        } else {
            gutter::build(
                &row_offsets,
                offset_width(self.document.original_len()),
                &self.palette,
            )
        };
        let hex_lines = if self.document.original_len() == 0 {
            vec![Line::raw("No content")]
        } else {
            hex_grid::build(
                &rows,
                &row_offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
            )
        };
        let ascii_lines = if self.document.original_len() == 0 {
            vec![Line::raw("")]
        } else {
            ascii_grid::build(
                &rows,
                &row_offsets,
                self.cursor,
                self.mode,
                &self.palette,
                self.config.bytes_per_line,
            )
        };

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
    }

    fn render_status(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let path_display = self.document.path().to_string_lossy();
        let line = status::build(
            status::StatusInfo {
                mode: self.mode,
                path: &path_display,
                cursor: self.cursor,
                len: self.document.original_len(),
                dirty: self.document.is_dirty(),
                message: &self.status_message,
                readonly: self.document.is_readonly(),
            },
            &self.palette,
        );
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_command(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let widget = command_line::widget(&self.command_buffer, &self.palette);
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
                self.mode = Mode::Command;
                self.command_buffer.clear();
                Ok(())
            }
            Action::LeaveMode => {
                self.mode = Mode::Normal;
                Ok(())
            }
            Action::DeleteByte => self.delete_current(),
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
                self.mode = Mode::Normal;
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

    fn delete_current(&mut self) -> HxResult<()> {
        self.document.delete_byte(self.cursor)?;
        self.status_message = format!("deleted 0x{:x}", self.cursor);
        Ok(())
    }

    fn edit_nibble(&mut self, value: u8) -> HxResult<()> {
        let phase = match self.mode {
            Mode::EditHex { phase } => phase,
            _ => return Ok(()),
        };
        self.document.replace_nibble(self.cursor, phase, value)?;
        self.status_message = format!("edited 0x{:x}", self.cursor);
        self.mode = match phase {
            NibblePhase::High => Mode::EditHex {
                phase: NibblePhase::Low,
            },
            NibblePhase::Low => {
                self.cursor = self.offset_with_delta(self.cursor, 1);
                Mode::EditHex {
                    phase: NibblePhase::High,
                }
            }
        };
        Ok(())
    }

    fn submit_command(&mut self) -> HxResult<()> {
        let command = parse_command(&self.command_buffer)?;
        self.command_buffer.clear();
        self.mode = Mode::Normal;
        self.execute_command(command)
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
                self.cursor = self.clamp_offset(self.cursor);
                self.status_message = format!("wrote {}", saved.display());
                Ok(())
            }
            Command::WriteQuit { path } => {
                let saved = self.document.save(path)?;
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
            Command::SearchAscii { pattern } | Command::SearchHex { pattern } => {
                // Search continues after the current cursor, mirroring modal
                // editor behavior and avoiding immediate self-matches.
                let start = if self.document.original_len() == 0 {
                    0
                } else {
                    (self.cursor + 1).min(self.document.original_len())
                };
                if let Some(found) = self.document.search_forward(start, &pattern)? {
                    self.cursor = found;
                    self.status_message = format!("found at 0x{:x}", found);
                } else {
                    self.status_message = "pattern not found".to_owned();
                }
                Ok(())
            }
        }
    }

    fn ensure_cursor_visible(&mut self) {
        let row_size = self.config.bytes_per_line as u64;
        let cursor_row = align_offset(self.cursor, self.config.bytes_per_line);
        let visible_rows = self.view_rows.max(1) as u64;
        let bottom = self.viewport_top + visible_rows.saturating_sub(1) * row_size;
        if cursor_row < self.viewport_top {
            self.viewport_top = cursor_row;
        } else if cursor_row > bottom {
            self.viewport_top =
                cursor_row.saturating_sub((visible_rows.saturating_sub(1)) * row_size);
        }
        self.viewport_top = align_offset(self.viewport_top, self.config.bytes_per_line);
    }

    fn offset_with_delta(&self, current: u64, delta: i64) -> u64 {
        if self.document.original_len() == 0 {
            return 0;
        }
        let max = self.document.original_len() - 1;
        if delta >= 0 {
            current.saturating_add(delta as u64).min(max)
        } else {
            current.saturating_sub(delta.unsigned_abs()).min(max)
        }
    }

    fn clamp_offset(&self, offset: u64) -> u64 {
        if self.document.original_len() == 0 {
            0
        } else {
            offset.min(self.document.original_len() - 1)
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
