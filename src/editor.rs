//! Editor state and input handling.
//!
//! Everything in this module is pure logic: no terminal I/O, no rendering.
//! [`Editor::handle_input`] takes an [`Input`] and returns an [`Effect`],
//! which the caller (main loop) acts on. This makes the whole editor
//! testable without a terminal.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Width in columns used to render tab characters.
pub const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Command,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Command => "COMMAND",
        }
    }
}

/// A decoded keystroke, independent of the terminal backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A printable character (includes space and tab when typed).
    Char(char),
    /// A control combination, e.g. Ctrl+C.
    Ctrl(char),
    Enter,
    Backspace,
    Delete,
    Esc,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
}

/// The outcome of handling an input, acted on by the main loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing special; status message (if any) is already set.
    None,
    /// The editor saved to this path.
    Saved(PathBuf),
    /// The editor wants to quit.
    Quit,
}

pub struct Editor {
    pub file_path: Option<PathBuf>,
    /// Buffer lines. Each line is a `Vec<char>` so multi-byte characters and
    /// emoji can never split a cursor position mid-codepoint.
    pub lines: Vec<Vec<char>>,
    /// Cursor column (char index into the current line).
    pub cx: usize,
    /// Cursor row (index into `lines`).
    pub cy: usize,
    pub mode: Mode,
    /// Buffer while typing after `:`.
    pub command: String,
    /// Message shown on the last terminal row.
    pub status: String,
    pub dirty: bool,
    pub screen_rows: usize,
    pub screen_cols: usize,
    /// First visible buffer row (vertical scroll).
    pub row_off: usize,
    /// First visible buffer column (horizontal scroll).
    pub col_off: usize,
    /// Normal mode: `d` was pressed, waiting for the second `d`.
    pending_d: bool,
}

impl Editor {
    pub fn new(file_path: Option<PathBuf>, screen_rows: usize, screen_cols: usize) -> Editor {
        Editor {
            file_path,
            lines: vec![Vec::new()],
            cx: 0,
            cy: 0,
            mode: Mode::Normal,
            command: String::new(),
            status: String::from(WELCOME),
            dirty: false,
            screen_rows,
            screen_cols,
            row_off: 0,
            col_off: 0,
            pending_d: false,
        }
    }

    /// Open a file, or start an empty buffer if it doesn't exist yet.
    /// Other I/O errors are returned to the caller.
    pub fn open(path: &Path, screen_rows: usize, screen_cols: usize) -> io::Result<Editor> {
        let mut ed = Editor::new(Some(path.to_path_buf()), screen_rows, screen_cols);
        match fs::read_to_string(path) {
            Ok(content) => {
                ed.lines = split_lines(&content);
                ed.status = format!("Opened {} ({} lines)", path.display(), ed.lines.len());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                ed.status = format!("New file: {}", path.display());
            }
            Err(e) => return Err(e),
        }
        Ok(ed)
    }

    /// Construct an editor from in-memory content (used by tests).
    pub fn from_content(
        content: &str,
        file_path: Option<PathBuf>,
        screen_rows: usize,
        screen_cols: usize,
    ) -> Editor {
        let mut ed = Editor::new(file_path, screen_rows, screen_cols);
        ed.lines = split_lines(content);
        ed
    }

    // ------------------------------------------------------------------
    // Small accessors / invariants
    // ------------------------------------------------------------------

    pub fn cur_line_len(&self) -> usize {
        self.lines[self.cy].len()
    }

    /// In NORMAL mode the cursor sits on an existing character (vim-style),
    /// so its maximum column is `len - 1` (0 on an empty line).
    fn clamp_cx_normal(&mut self) {
        self.cx = self.cx.min(self.cur_line_len().saturating_sub(1));
    }

    /// Restore cursor invariants after any input.
    fn clamp_cursor(&mut self) {
        self.cy = self.cy.min(self.lines.len() - 1);
        let len = self.cur_line_len();
        match self.mode {
            // INSERT may sit one past the last char (append position);
            // COMMAND's cursor lives on the status bar, clamping is harmless.
            Mode::Insert | Mode::Command => self.cx = self.cx.min(len),
            Mode::Normal => self.clamp_cx_normal(),
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    // ------------------------------------------------------------------
    // Input dispatch
    // ------------------------------------------------------------------

    pub fn handle_input(&mut self, input: Input) -> Effect {
        let effect = match self.mode {
            Mode::Normal => self.normal_input(input),
            Mode::Insert => {
                self.insert_input(input);
                Effect::None
            }
            Mode::Command => self.command_input(input),
        };
        self.clamp_cursor();
        effect
    }

    fn normal_input(&mut self, input: Input) -> Effect {
        // `dd` deletes the current line; any other key clears the pending state.
        if self.pending_d {
            self.pending_d = false;
            if input == Input::Char('d') {
                self.delete_line();
                return Effect::None;
            }
        }

        match input {
            Input::Char('h') | Input::ArrowLeft => {
                if self.cx > 0 {
                    self.cx -= 1;
                }
            }
            Input::Char('l') | Input::ArrowRight => {
                if self.cx < self.cur_line_len().saturating_sub(1) {
                    self.cx += 1;
                }
            }
            Input::Char('j') | Input::ArrowDown | Input::Enter => {
                if self.cy + 1 < self.lines.len() {
                    self.cy += 1;
                    self.clamp_cx_normal();
                    if input == Input::Enter {
                        self.cx = 0;
                    }
                }
            }
            Input::Char('k') | Input::ArrowUp => {
                if self.cy > 0 {
                    self.cy -= 1;
                    self.clamp_cx_normal();
                }
            }
            Input::Char('0') => self.cx = 0,
            Input::Char('$') | Input::End => {
                self.cx = self.cur_line_len().saturating_sub(1);
            }
            Input::Home => self.cx = 0,
            Input::Char('g') => {
                self.cy = 0;
                self.clamp_cx_normal();
            }
            Input::Char('G') => {
                self.cy = self.lines.len() - 1;
                self.clamp_cx_normal();
            }
            Input::PageUp => {
                let page = self.page_height();
                self.cy = self.cy.saturating_sub(page);
                self.clamp_cx_normal();
            }
            Input::PageDown => {
                let page = self.page_height();
                self.cy = (self.cy + page).min(self.lines.len() - 1);
                self.clamp_cx_normal();
            }
            Input::Char(':') => {
                self.mode = Mode::Command;
                self.command.clear();
            }
            Input::Char('i') => {
                self.mode = Mode::Insert;
                self.status = String::from("-- INSERT -- (Esc for NORMAL)");
            }
            Input::Char('a') => {
                if self.cur_line_len() > 0 {
                    self.cx += 1;
                }
                self.mode = Mode::Insert;
                self.status = String::from("-- INSERT -- (Esc for NORMAL)");
            }
            Input::Char('A') => {
                self.cx = self.cur_line_len();
                self.mode = Mode::Insert;
                self.status = String::from("-- INSERT -- (Esc for NORMAL)");
            }
            Input::Char('o') => {
                self.lines.insert(self.cy + 1, Vec::new());
                self.cy += 1;
                self.cx = 0;
                self.mode = Mode::Insert;
                self.mark_dirty();
                self.status = String::from("-- INSERT -- (Esc for NORMAL)");
            }
            Input::Char('O') => {
                self.lines.insert(self.cy, Vec::new());
                self.cx = 0;
                self.mode = Mode::Insert;
                self.mark_dirty();
                self.status = String::from("-- INSERT -- (Esc for NORMAL)");
            }
            Input::Char('x') | Input::Delete => {
                if self.cx < self.cur_line_len() {
                    self.lines[self.cy].remove(self.cx);
                    self.mark_dirty();
                }
            }
            Input::Char('d') => {
                self.pending_d = true;
            }
            Input::Esc => {
                self.pending_d = false;
            }
            Input::Ctrl('c') => {
                self.status = String::from("Type :q to quit (or :q! to force)");
            }
            _ => {}
        }
        Effect::None
    }

    fn page_height(&self) -> usize {
        self.screen_rows.saturating_sub(2).max(1)
    }

    fn insert_input(&mut self, input: Input) {
        match input {
            Input::Esc | Input::Ctrl('c') => {
                self.mode = Mode::Normal;
                self.status = String::from(WELCOME);
                // Vim behaviour: leaving INSERT moves the cursor off the
                // past-the-end position back onto the last character.
                if self.cx > 0 && self.cx >= self.cur_line_len() {
                    self.cx = self.cur_line_len() - 1;
                }
            }
            Input::Enter => {
                let right = self.lines[self.cy][self.cx..].to_vec();
                let left = self.lines[self.cy][..self.cx].to_vec();
                self.lines[self.cy] = left;
                self.lines.insert(self.cy + 1, right);
                self.cy += 1;
                self.cx = 0;
                self.mark_dirty();
            }
            Input::Backspace => {
                if self.cx > 0 {
                    self.lines[self.cy].remove(self.cx - 1);
                    self.cx -= 1;
                } else if self.cy > 0 {
                    let cur = self.lines.remove(self.cy);
                    self.cy -= 1;
                    self.cx = self.lines[self.cy].len();
                    self.lines[self.cy].extend(cur);
                }
                self.mark_dirty();
            }
            Input::Delete => {
                if self.cx < self.cur_line_len() {
                    self.lines[self.cy].remove(self.cx);
                } else if self.cy + 1 < self.lines.len() {
                    let next = self.lines.remove(self.cy + 1);
                    self.lines[self.cy].extend(next);
                }
                self.mark_dirty();
            }
            Input::Tab => {
                self.insert_char('\t');
            }
            Input::Char(c) => {
                self.insert_char(c);
            }
            Input::ArrowLeft => {
                self.cx = self.cx.saturating_sub(1);
            }
            Input::ArrowRight => {
                if self.cx < self.cur_line_len() {
                    self.cx += 1;
                }
            }
            Input::ArrowUp => {
                if self.cy > 0 {
                    self.cy -= 1;
                    self.cx = self.cx.min(self.cur_line_len());
                }
            }
            Input::ArrowDown => {
                if self.cy + 1 < self.lines.len() {
                    self.cy += 1;
                    self.cx = self.cx.min(self.cur_line_len());
                }
            }
            Input::Home => self.cx = 0,
            Input::End => self.cx = self.cur_line_len(),
            _ => {}
        }
    }

    fn insert_char(&mut self, c: char) {
        self.lines[self.cy].insert(self.cx, c);
        self.cx += 1;
        self.mark_dirty();
    }

    fn command_input(&mut self, input: Input) -> Effect {
        match input {
            Input::Esc | Input::Ctrl('c') => {
                self.mode = Mode::Normal;
                self.command.clear();
            }
            Input::Enter => {
                self.mode = Mode::Normal;
                let effect = self.execute_command();
                self.command.clear();
                return effect;
            }
            Input::Backspace => {
                self.command.pop();
            }
            Input::Char(c) => self.command.push(c),
            _ => {}
        }
        Effect::None
    }

    // ------------------------------------------------------------------
    // Ex commands
    // ------------------------------------------------------------------

    fn execute_command(&mut self) -> Effect {
        let cmd = self.command.trim().to_string();
        let mut tokens = cmd.split_whitespace();
        let head = tokens.next().unwrap_or("").to_string();
        let arg: Option<String> = tokens.next().map(|s| s.to_string());

        match (head.as_str(), arg) {
            ("w", arg) => self.save_cmd(arg),
            ("q", None) => {
                if self.dirty {
                    self.status = String::from("No write since last change (add ! to override)");
                    Effect::None
                } else {
                    Effect::Quit
                }
            }
            ("q!", _) => Effect::Quit,
            ("wq", arg) | ("x", arg) => {
                let effect = self.save_cmd(arg);
                if matches!(effect, Effect::Saved(_)) {
                    Effect::Quit
                } else {
                    effect
                }
            }
            ("", _) => Effect::None, // bare `:` + Enter
            _ => {
                self.status = format!("Not an editor command: {cmd}");
                Effect::None
            }
        }
    }

    fn save_cmd(&mut self, arg: Option<String>) -> Effect {
        let path: PathBuf = match arg {
            Some(a) => {
                let p = PathBuf::from(a);
                self.file_path = Some(p.clone());
                p
            }
            None => match &self.file_path {
                Some(p) => p.clone(),
                None => {
                    self.status = String::from("No file name: use :w <filename>");
                    return Effect::None;
                }
            },
        };
        match self.save_to(&path) {
            Ok(()) => {
                self.status = format!("Wrote {} lines to {}", self.lines.len(), path.display());
                Effect::Saved(path)
            }
            Err(e) => {
                self.status = format!("Error saving {}: {e}", path.display());
                Effect::None
            }
        }
    }

    /// Write the buffer to `path` with a trailing newline (POSIX convention).
    fn save_to(&mut self, path: &Path) -> io::Result<()> {
        let mut out = String::with_capacity(self.buffer_size_hint());
        for line in &self.lines {
            out.extend(line.iter());
            out.push('\n');
        }
        fs::write(path, out)?;
        self.dirty = false;
        Ok(())
    }

    fn buffer_size_hint(&self) -> usize {
        self.lines
            .iter()
            .map(|l| l.len() + 1)
            .sum::<usize>()
            .max(16)
    }

    fn delete_line(&mut self) {
        if self.lines.len() > 1 {
            self.lines.remove(self.cy);
            self.cy = self.cy.min(self.lines.len() - 1);
        } else {
            self.lines[0].clear();
            self.cy = 0;
        }
        self.cx = 0;
        self.mark_dirty();
    }

    // ------------------------------------------------------------------
    // Scrolling
    // ------------------------------------------------------------------

    /// Adjust scroll offsets so the cursor is always visible.
    /// Called by the renderer before drawing.
    pub fn update_scroll(&mut self) {
        let text_rows = self.page_height();
        if self.cy < self.row_off {
            self.row_off = self.cy;
        }
        if self.cy >= self.row_off + text_rows {
            self.row_off = self.cy + 1 - text_rows;
        }

        let cols = self.screen_cols.max(1);
        let col = display_col(&self.lines[self.cy], self.cx);
        if col < self.col_off {
            self.col_off = col;
        }
        if col >= self.col_off + cols {
            self.col_off = col + 1 - cols;
        }
    }
}

pub const WELCOME: &str =
    "HELP: i insert | dd delete line | :w save | :q quit | :q! force quit | :wq save & quit";

// ----------------------------------------------------------------------
// Buffer parsing / display-width helpers
// ----------------------------------------------------------------------

/// Split file content into buffer lines. Handles a trailing newline and
/// normalizes CRLF to LF. An empty file is one empty line.
pub fn split_lines(content: &str) -> Vec<Vec<char>> {
    let content = content.strip_suffix('\n').unwrap_or(content);
    if content.is_empty() {
        return vec![Vec::new()];
    }
    content
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).chars().collect())
        .collect()
}

/// Display width of a single character as rendered by [`crate::ui`].
pub fn char_display_width(c: char) -> usize {
    if c == '\t' {
        TAB_WIDTH
    } else {
        unicode_width::UnicodeWidthChar::width(c).unwrap_or(1)
    }
}

/// Display column of cursor position `cx` within `line`
/// (tabs count as [`TAB_WIDTH`] columns).
pub fn display_col(line: &[char], cx: usize) -> usize {
    line.iter().take(cx).map(|&c| char_display_width(c)).sum()
}
