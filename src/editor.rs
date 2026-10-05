//! Editor state and input handling.
//!
//! Everything in this module is pure logic: no terminal I/O, no rendering.
//! [`Editor::handle_input`] takes an [`Input`] and returns an [`Effect`],
//! which the caller (main loop) acts on. This makes the whole editor
//! testable without a terminal.
//!
//! Mutations are recorded as [`Change`]s grouped into [`Transaction`]s,
//! which gives undo/redo for free and drives the dirty flag (`dirty()`
//! is simply "is the undo stack deeper than it was at the last save").

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::syntax::Lang;

/// Width in columns used to render tab characters.
pub const TAB_WIDTH: usize = 4;

/// Maximum number of undo transactions kept.
const MAX_UNDO: usize = 1000;

/// Largest payload sent to the system clipboard via OSC 52.
const MAX_OSC52_BYTES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Command,
    /// Typing a `/` search term.
    Search,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
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

/// A single atomic buffer mutation. Applying it forward and reverting it
/// (in reverse order within a transaction) are exact inverses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Text inserted into `line` starting at char column `col`.
    Insert {
        line: usize,
        col: usize,
        text: Vec<char>,
    },
    /// Text deleted from `line` starting at char column `col`.
    Delete {
        line: usize,
        col: usize,
        text: Vec<char>,
    },
    /// A new line inserted at `index`.
    LineInsert { index: usize, line: Vec<char> },
    /// The line at `index` was removed (`line` holds its old content).
    LineDelete { index: usize, line: Vec<char> },
}

/// One user-visible edit step: all changes from a single input, plus the
/// cursor position before and after, so undo/redo can restore it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    changes: Vec<Change>,
    before: (usize, usize), // (cx, cy)
    after: (usize, usize),
}

/// What `p`/`P` paste. Vim-style: line registers paste as whole lines,
/// char registers paste inline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Register {
    Lines(Vec<Vec<char>>),
    Chars(Vec<char>),
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
    /// Buffer while typing after `/`.
    pub search_term: String,
    /// Last executed search, reused by `n`/`N` and used for highlighting.
    pub last_search: Option<String>,
    /// Message shown on the last terminal row.
    pub status: String,
    /// Transient messages fade after `status_timeout`; help messages stay.
    status_permanent: bool,
    status_set_at: Option<Instant>,
    pub status_timeout: Duration,
    /// `/`-search matches are highlighted until the buffer is edited.
    highlight_active: bool,
    pub screen_rows: usize,
    pub screen_cols: usize,
    /// First visible buffer row (vertical scroll).
    pub row_off: usize,
    /// First visible buffer column (horizontal scroll).
    pub col_off: usize,
    /// Normal mode: `d` was pressed, waiting for the second `d`.
    pending_d: bool,
    /// Normal mode: `y` was pressed, waiting for the second `y`.
    pending_y: bool,
    /// Unnamed register used by `p`/`P`.
    register: Option<Register>,
    /// Detected language for syntax highlighting.
    pub lang: Option<Lang>,
    /// Text queued for the system clipboard (OSC 52), taken by the main loop.
    pending_clipboard: Option<String>,
    /// Undo history (oldest first).
    undo_stack: Vec<Transaction>,
    /// Redo history (most recent first).
    redo_stack: Vec<Transaction>,
    /// `undo_stack.len()` at the last successful save; dirty() compares.
    saved_undo_len: usize,
    /// Set by save so the next commit starts a fresh transaction instead of
    /// coalescing into a pre-save one (keeps dirty() exact).
    coalesce_barrier: bool,
    /// Set by `o`/`O`: text typed right after opening a line joins the same
    /// undo transaction, so one `u` removes the opened line with its text.
    open_line_pending: bool,
}

impl Editor {
    pub fn new(file_path: Option<PathBuf>, screen_rows: usize, screen_cols: usize) -> Editor {
        let lang = file_path.as_deref().and_then(Lang::from_path);
        Editor {
            file_path,
            lines: vec![Vec::new()],
            cx: 0,
            cy: 0,
            mode: Mode::Normal,
            command: String::new(),
            search_term: String::new(),
            last_search: None,
            status: String::from(WELCOME),
            status_permanent: true,
            status_set_at: None,
            status_timeout: Duration::from_secs(3),
            highlight_active: false,
            screen_rows,
            screen_cols,
            row_off: 0,
            col_off: 0,
            pending_d: false,
            pending_y: false,
            register: None,
            lang,
            pending_clipboard: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            saved_undo_len: 0,
            coalesce_barrier: false,
            open_line_pending: false,
        }
    }

    /// Open a file, or start an empty buffer if it doesn't exist yet.
    /// Other I/O errors are returned to the caller.
    pub fn open(path: &Path, screen_rows: usize, screen_cols: usize) -> io::Result<Editor> {
        let mut ed = Editor::new(Some(path.to_path_buf()), screen_rows, screen_cols);
        match fs::read_to_string(path) {
            Ok(content) => {
                ed.lines = split_lines(&content);
                ed.set_status(format!(
                    "Opened {} ({} lines)",
                    path.display(),
                    ed.lines.len()
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                ed.set_status(format!("New file: {}", path.display()));
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

    /// True when the buffer differs from the last saved state. Derived from
    /// the undo stack, so undoing back to the saved state also clears it.
    pub fn dirty(&self) -> bool {
        self.undo_stack.len() != self.saved_undo_len
    }

    /// Show a transient message on the status line (fades after
    /// [`Editor::status_timeout`]).
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_permanent = false;
        self.status_set_at = Some(Instant::now());
    }

    /// Clear the status message if it is a transient one whose time is up.
    /// Returns true when the message changed (caller should redraw).
    pub fn tick_status(&mut self) -> bool {
        if self.status_permanent || self.status.is_empty() {
            return false;
        }
        let expired = self
            .status_set_at
            .map(|t| t.elapsed() >= self.status_timeout)
            .unwrap_or(false);
        if expired {
            self.status.clear();
            self.status_set_at = None;
            true
        } else {
            false
        }
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
            // COMMAND/SEARCH cursors live on the status bar, clamping is harmless.
            Mode::Insert | Mode::Command | Mode::Search => self.cx = self.cx.min(len),
            Mode::Normal => self.clamp_cx_normal(),
        }
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
            Mode::Search => {
                self.search_input(input);
                Effect::None
            }
        };
        self.clamp_cursor();
        effect
    }

    fn normal_input(&mut self, input: Input) -> Effect {
        // `dd` / `yy` operator handling; any other key clears the pending state.
        if self.pending_d {
            self.pending_d = false;
            if input == Input::Char('d') {
                self.delete_line();
                return Effect::None;
            }
        }
        if self.pending_y {
            self.pending_y = false;
            if input == Input::Char('y') {
                self.yank_line();
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
            Input::Char('i') => {
                self.mode = Mode::Insert;
                self.set_status(String::from("-- INSERT -- (Esc for NORMAL)"));
            }
            Input::Char('a') => {
                if self.cur_line_len() > 0 {
                    self.cx += 1;
                }
                self.mode = Mode::Insert;
                self.set_status(String::from("-- INSERT -- (Esc for NORMAL)"));
            }
            Input::Char('A') => {
                self.cx = self.cur_line_len();
                self.mode = Mode::Insert;
                self.set_status(String::from("-- INSERT -- (Esc for NORMAL)"));
            }
            Input::Char('o') => {
                let before = (self.cx, self.cy);
                let index = self.cy + 1;
                self.lines.insert(index, Vec::new());
                self.cy = index;
                self.cx = 0;
                self.mode = Mode::Insert;
                self.commit(
                    before,
                    vec![Change::LineInsert {
                        index,
                        line: Vec::new(),
                    }],
                );
                // Redo should place the cursor on the opened line, not where
                // the cursor happened to sit when the change committed.
                if let Some(tx) = self.undo_stack.last_mut() {
                    tx.after = (0, index);
                }
                self.open_line_pending = true;
                self.set_status(String::from("-- INSERT -- (Esc for NORMAL)"));
            }
            Input::Char('O') => {
                let before = (self.cx, self.cy);
                let index = self.cy;
                self.lines.insert(index, Vec::new());
                self.cx = 0;
                self.mode = Mode::Insert;
                self.commit(
                    before,
                    vec![Change::LineInsert {
                        index,
                        line: Vec::new(),
                    }],
                );
                if let Some(tx) = self.undo_stack.last_mut() {
                    tx.after = (0, index);
                }
                self.open_line_pending = true;
                self.set_status(String::from("-- INSERT -- (Esc for NORMAL)"));
            }
            Input::Char('x') | Input::Delete => {
                if self.cx < self.cur_line_len() {
                    let before = (self.cx, self.cy);
                    let ch = self.lines[self.cy].remove(self.cx);
                    self.register = Some(Register::Chars(vec![ch]));
                    self.commit(
                        before,
                        vec![Change::Delete {
                            line: before.1,
                            col: before.0,
                            text: vec![ch],
                        }],
                    );
                }
            }
            Input::Char('d') => {
                self.pending_d = true;
                self.pending_y = false;
            }
            Input::Char('y') => {
                self.pending_y = true;
                self.pending_d = false;
            }
            Input::Char('p') => self.paste(false),
            Input::Char('P') => self.paste(true),
            Input::Char('u') => self.undo(),
            Input::Ctrl('r') => self.redo(),
            Input::Char(':') => {
                self.mode = Mode::Command;
                self.command.clear();
            }
            Input::Char('/') => {
                self.mode = Mode::Search;
                self.search_term.clear();
            }
            Input::Char('n') => self.jump_to_match(true),
            Input::Char('N') => self.jump_to_match(false),
            Input::Esc => {
                self.pending_d = false;
                self.pending_y = false;
                self.open_line_pending = false;
            }
            Input::Ctrl('c') => {
                self.set_status(String::from("Type :q to quit (or :q! to force)"));
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
                self.set_status(String::from(WELCOME));
                self.open_line_pending = false;
                // Vim behaviour: leaving INSERT moves the cursor off the
                // past-the-end position back onto the last character.
                if self.cx > 0 && self.cx >= self.cur_line_len() {
                    self.cx = self.cur_line_len() - 1;
                }
            }
            Input::Enter => {
                let before = (self.cx, self.cy);
                let right = self.lines[self.cy][self.cx..].to_vec();
                self.lines[self.cy].truncate(self.cx);
                self.lines.insert(self.cy + 1, right.clone());
                let index = self.cy;
                self.cy += 1;
                self.cx = 0;
                self.commit(
                    before,
                    vec![
                        Change::Delete {
                            line: index,
                            col: before.0,
                            text: right.clone(),
                        },
                        Change::LineInsert {
                            index: index + 1,
                            line: right,
                        },
                    ],
                );
            }
            Input::Backspace => {
                if self.cx > 0 {
                    let before = (self.cx, self.cy);
                    let col = self.cx - 1;
                    let ch = self.lines[self.cy].remove(col);
                    self.cx = col;
                    self.commit(
                        before,
                        vec![Change::Delete {
                            line: before.1,
                            col,
                            text: vec![ch],
                        }],
                    );
                } else if self.cy > 0 {
                    let before = (self.cx, self.cy);
                    let cur = self.lines.remove(self.cy);
                    self.cy -= 1;
                    let join_col = self.lines[self.cy].len();
                    self.cx = join_col;
                    self.lines[self.cy].extend(cur.iter().copied());
                    self.commit(
                        before,
                        vec![
                            Change::Insert {
                                line: self.cy,
                                col: join_col,
                                text: cur.clone(),
                            },
                            Change::LineDelete {
                                index: before.1,
                                line: cur,
                            },
                        ],
                    );
                }
            }
            Input::Delete => {
                if self.cx < self.cur_line_len() {
                    let before = (self.cx, self.cy);
                    let ch = self.lines[self.cy].remove(self.cx);
                    self.commit(
                        before,
                        vec![Change::Delete {
                            line: before.1,
                            col: before.0,
                            text: vec![ch],
                        }],
                    );
                } else if self.cy + 1 < self.lines.len() {
                    let before = (self.cx, self.cy);
                    let next = self.lines.remove(self.cy + 1);
                    let join_col = self.lines[self.cy].len();
                    self.lines[self.cy].extend(next.iter().copied());
                    self.commit(
                        before,
                        vec![
                            Change::Insert {
                                line: self.cy,
                                col: join_col,
                                text: next.clone(),
                            },
                            Change::LineDelete {
                                index: self.cy + 1,
                                line: next,
                            },
                        ],
                    );
                }
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
        let before = (self.cx, self.cy);
        let (line, col) = (self.cy, self.cx);
        self.lines[line].insert(col, c);
        self.cx += 1;
        self.commit(
            before,
            vec![Change::Insert {
                line,
                col,
                text: vec![c],
            }],
        );
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

    fn search_input(&mut self, input: Input) {
        match input {
            Input::Esc | Input::Ctrl('c') => {
                self.mode = Mode::Normal;
                self.search_term.clear();
            }
            Input::Enter => {
                if !self.search_term.is_empty() {
                    self.last_search = Some(self.search_term.clone());
                    self.jump_to_match(true);
                }
                self.mode = Mode::Normal;
                self.search_term.clear();
            }
            Input::Backspace => {
                self.search_term.pop();
            }
            Input::Char(c) => self.search_term.push(c),
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // Undo / redo
    // ------------------------------------------------------------------

    /// Record a completed mutation group. Consecutive single-char inserts at
    /// the cursor position coalesce into one transaction (vim-like grouping),
    /// text typed right after `o`/`O` joins the open-line transaction, and a
    /// save barrier forces a fresh transaction (keeps dirty() exact).
    fn commit(&mut self, before: (usize, usize), changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        let after = (self.cx, self.cy);
        self.redo_stack.clear();
        // Editing the buffer clears `/`-search highlighting (vim hlsearch).
        self.highlight_active = false;

        if !self.coalesce_barrier && changes.len() == 1 {
            if let Some(last) = self.undo_stack.last_mut() {
                if last.after == before {
                    let joins = match last.changes.last_mut() {
                        // "abc" typing session: each char lands right after
                        // the previous one on the same line.
                        Some(Change::Insert {
                            line: l2,
                            col: c2,
                            text: t2,
                        }) => {
                            matches!(changes[0], Change::Insert { line, col, .. }
                                if *l2 == line && *c2 + t2.len() == col)
                        }
                        // `o`/`O` + typed text on the freshly opened line.
                        Some(Change::LineInsert { index, .. }) => {
                            self.open_line_pending
                                && matches!(changes[0], Change::Insert { .. } if *index == before.1)
                        }
                        _ => false,
                    };
                    if joins {
                        last.changes.push(changes.into_iter().next().unwrap());
                        last.after = after;
                        self.open_line_pending = false;
                        return;
                    }
                }
            }
        }
        self.open_line_pending = false;
        self.coalesce_barrier = false;
        self.undo_stack.push(Transaction {
            changes,
            before,
            after,
        });
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
            self.saved_undo_len = self.saved_undo_len.saturating_sub(1);
        }
    }

    pub fn undo(&mut self) {
        let Some(tx) = self.undo_stack.pop() else {
            self.set_status(String::from("Already at oldest change"));
            return;
        };
        for change in tx.changes.iter().rev() {
            self.revert(change);
        }
        (self.cx, self.cy) = tx.before;
        self.redo_stack.push(tx.clone());
        self.set_status(format!("Undid {} change(s)", tx.changes.len()));
    }

    pub fn redo(&mut self) {
        let Some(tx) = self.redo_stack.pop() else {
            self.set_status(String::from("Already at newest change"));
            return;
        };
        for change in &tx.changes {
            self.apply(change);
        }
        (self.cx, self.cy) = tx.after;
        self.undo_stack.push(tx.clone());
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
            self.saved_undo_len = self.saved_undo_len.saturating_sub(1);
        }
        self.set_status(format!("Redid {} change(s)", tx.changes.len()));
    }

    fn apply(&mut self, change: &Change) {
        match change {
            Change::Insert { line, col, text } => {
                let l = &mut self.lines[*line];
                for (i, c) in text.iter().enumerate() {
                    l.insert(col + i, *c);
                }
            }
            Change::Delete { line, col, text } => {
                self.lines[*line].drain(*col..*col + text.len());
            }
            Change::LineInsert { index, line } => {
                self.lines.insert(*index, line.clone());
            }
            Change::LineDelete { index, .. } => {
                self.lines.remove(*index);
            }
        }
    }

    fn revert(&mut self, change: &Change) {
        match change {
            Change::Insert { line, col, text } => {
                self.lines[*line].drain(*col..*col + text.len());
            }
            Change::Delete { line, col, text } => {
                let l = &mut self.lines[*line];
                for (i, c) in text.iter().enumerate() {
                    l.insert(col + i, *c);
                }
            }
            Change::LineInsert { index, .. } => {
                self.lines.remove(*index);
            }
            Change::LineDelete { index, line } => {
                self.lines.insert(*index, line.clone());
            }
        }
    }

    // ------------------------------------------------------------------
    // Yank / paste / registers
    // ------------------------------------------------------------------

    /// Text queued for the system clipboard; the main loop sends it as OSC 52.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.pending_clipboard.take()
    }

    fn set_clipboard(&mut self, text: String) {
        if !text.is_empty() && text.len() <= MAX_OSC52_BYTES {
            self.pending_clipboard = Some(text);
        }
    }

    fn yank_line(&mut self) {
        let line = self.lines[self.cy].clone();
        let text: String = line.iter().collect();
        self.register = Some(Register::Lines(vec![line]));
        self.set_clipboard(text);
        self.set_status(format!(
            "Yanked line {} (also copied via OSC 52)",
            self.cy + 1
        ));
    }

    fn paste(&mut self, before: bool) {
        let reg = match &self.register {
            Some(r) => r.clone(),
            None => {
                self.set_status(String::from("Nothing to paste"));
                return;
            }
        };
        let before_pos = (self.cx, self.cy);
        match reg {
            Register::Lines(lines) => {
                let n = lines.len();
                let start = if before { self.cy } else { self.cy + 1 };
                for (i, l) in lines.iter().enumerate() {
                    self.lines.insert(start + i, l.clone());
                }
                let changes: Vec<Change> = lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| Change::LineInsert {
                        index: start + i,
                        line: l.clone(),
                    })
                    .collect();
                self.cy = start;
                self.cx = 0;
                self.commit(before_pos, changes);
                self.set_status(format!("Pasted {n} line(s)"));
            }
            Register::Chars(chars) => {
                let n = chars.len();
                let len = self.cur_line_len();
                let col = if before {
                    self.cx
                } else if len > 0 {
                    // vim `p` pastes after the char under the cursor
                    (self.cx + 1).min(len)
                } else {
                    0
                };
                self.lines[self.cy].splice(col..col, chars.iter().copied());
                self.cx = col + n;
                self.commit(
                    before_pos,
                    vec![Change::Insert {
                        line: before_pos.1,
                        col,
                        text: chars,
                    }],
                );
                self.set_status(format!("Pasted {n} char(s)"));
            }
        }
    }

    // ------------------------------------------------------------------
    // Search
    // ------------------------------------------------------------------

    /// All non-overlapping match start positions of `term` in `line`.
    /// Smart-case: the term is case-insensitive unless it contains uppercase.
    pub fn find_matches(line: &[char], term: &str) -> Vec<usize> {
        let t: Vec<char> = term.chars().collect();
        if t.is_empty() || line.len() < t.len() {
            return Vec::new();
        }
        let case_sensitive = t.iter().any(|c| c.is_uppercase());
        let eq = |a: char, b: char| {
            if case_sensitive {
                a == b
            } else {
                a.to_lowercase().eq(b.to_lowercase())
            }
        };
        let mut out = Vec::new();
        let mut start = 0;
        while start + t.len() <= line.len() {
            if (0..t.len()).all(|k| eq(line[start + k], t[k])) {
                out.push(start);
                start += t.len();
            } else {
                start += 1;
            }
        }
        out
    }

    /// Match ranges (start, end) in buffer line `line_idx`, for highlighting.
    pub fn search_ranges(&self, line_idx: usize) -> Vec<(usize, usize)> {
        // Matches highlight from the moment a search runs until the buffer
        // is edited (vim hlsearch behaviour).
        if !self.highlight_active {
            return Vec::new();
        }
        let Some(term) = &self.last_search else {
            return Vec::new();
        };
        if term.is_empty() {
            return Vec::new();
        }
        let tlen = term.chars().count();
        Self::find_matches(&self.lines[line_idx], term)
            .into_iter()
            .map(|s| (s, s + tlen))
            .collect()
    }

    /// Jump to the next (`forward`) or previous match of the last search,
    /// wrapping around the buffer.
    fn jump_to_match(&mut self, forward: bool) {
        let Some(term) = self.last_search.clone() else {
            self.set_status(String::from("No previous search"));
            return;
        };
        if term.is_empty() {
            return;
        }
        self.highlight_active = true;
        let n = self.lines.len();
        let cx = self.cx;
        // Current line: respect direction relative to the cursor. Other
        // lines: any match counts (we wrap around the whole buffer).
        let pick_current = |line: &[char]| -> Option<(usize, usize)> {
            let matches = Self::find_matches(line, &term);
            let col = if forward {
                matches.into_iter().find(|s| *s > cx)
            } else {
                matches.into_iter().rfind(|s| *s < cx)
            };
            col.map(|c| (self.cy, c))
        };
        let pick_other = |line: &[char], idx: usize| -> Option<(usize, usize)> {
            let matches = Self::find_matches(line, &term);
            let col = if forward {
                matches.into_iter().next()
            } else {
                matches.into_iter().next_back()
            };
            col.map(|c| (idx, c))
        };

        let mut hit = pick_current(&self.lines[self.cy]);
        if hit.is_none() {
            for i in 1..n {
                let idx = if forward {
                    (self.cy + i) % n
                } else {
                    (self.cy + n - i) % n
                };
                hit = pick_other(&self.lines[idx], idx);
                if hit.is_some() {
                    break;
                }
            }
        }

        match hit {
            Some((y, x)) => {
                self.cy = y;
                self.cx = x;
                self.set_status(format!("/{}", term));
            }
            None => {
                self.set_status(format!("Pattern not found: {}", term));
            }
        }
    }

    // ------------------------------------------------------------------
    // Ex commands
    // ------------------------------------------------------------------

    fn execute_command(&mut self) -> Effect {
        let cmd = self.command.trim().to_string();
        // Substitute commands contain the delimiter in the head itself
        // (`s/../../g`), so they're matched before whitespace tokenizing.
        if cmd == "s" || cmd.starts_with("s/") {
            return self.substitute_cmd(&cmd, false);
        }
        if cmd.starts_with("%s/") {
            return self.substitute_cmd(&cmd, true);
        }
        let mut tokens = cmd.split_whitespace();
        let head = tokens.next().unwrap_or("").to_string();
        let arg: Option<String> = tokens.next().map(|s| s.to_string());

        match (head.as_str(), arg) {
            ("w", arg) => self.save_cmd(arg),
            ("q", None) => {
                if self.dirty() {
                    self.set_status(String::from(
                        "No write since last change (add ! to override)",
                    ));
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
            ("undo", _) => {
                self.undo();
                Effect::None
            }
            ("redo", _) => {
                self.redo();
                Effect::None
            }
            ("s", _) | ("%s", _) => self.substitute_cmd(&cmd, head == "%s"),
            ("", _) => Effect::None, // bare `:` + Enter
            _ => {
                self.set_status(format!("Not an editor command: {cmd}"));
                Effect::None
            }
        }
    }

    /// Parse `s/old/new/flags` (or `%s/...`). Returns (old, new, global).
    /// `\/` and `\\` are unescaped in both parts; an empty `old` reuses the
    /// last `/`-search term.
    pub fn parse_substitute(body: &str) -> Result<(String, String, bool), String> {
        let mut chars = body.chars().peekable();
        if chars.next() != Some('/') {
            return Err(String::from("Usage: s/old/new/ (or :%s/old/new/g)"));
        }
        let mut part = String::new();
        let mut parts: Vec<String> = Vec::new();
        let mut escaped = false;
        for c in chars {
            if escaped {
                match c {
                    '/' => part.push('/'),
                    '\\' => part.push('\\'),
                    other => {
                        part.push('\\');
                        part.push(other);
                    }
                }
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '/' {
                parts.push(std::mem::take(&mut part));
            } else {
                part.push(c);
            }
        }
        if escaped {
            part.push('\\');
        }
        parts.push(part); // replacement (may lack trailing '/')

        match parts.len() {
            1 => Err(String::from("Usage: s/old/new/")),
            _ => {
                let global = parts.len() > 2 && parts[2].contains('g');
                Ok((parts[0].clone(), parts[1].clone(), global))
            }
        }
    }

    fn substitute_cmd(&mut self, cmd: &str, all_lines: bool) -> Effect {
        let body = cmd
            .trim()
            .strip_prefix(if all_lines { "%s" } else { "s" })
            .unwrap_or("");
        let (old, new, global) = match Self::parse_substitute(body) {
            Ok(parsed) => parsed,
            Err(msg) => {
                self.set_status(msg);
                return Effect::None;
            }
        };
        // Empty pattern: reuse the last search (vim behaviour).
        let old = if old.is_empty() {
            match &self.last_search {
                Some(t) if !t.is_empty() => t.clone(),
                _ => {
                    self.set_status(String::from("No previous search"));
                    return Effect::None;
                }
            }
        } else {
            old
        };

        let before = (self.cx, self.cy);
        let mut changes: Vec<Change> = Vec::new();
        let mut hits = 0usize;
        let mut hit_lines = 0usize;

        let line_range: Vec<usize> = if all_lines {
            (0..self.lines.len()).collect()
        } else {
            vec![self.cy]
        };

        for line_idx in line_range {
            let mut spans = Self::find_matches(&self.lines[line_idx], &old);
            if !global {
                spans.truncate(1);
            }
            if spans.is_empty() {
                continue;
            }
            hits += spans.len();
            hit_lines += 1;
            // Right-to-left keeps earlier match positions valid.
            let new_chars: Vec<char> = new.chars().collect();
            for start in spans.into_iter().rev() {
                let old_chars: Vec<char> = old.chars().collect();
                changes.push(Change::Delete {
                    line: line_idx,
                    col: start,
                    text: old_chars,
                });
                changes.push(Change::Insert {
                    line: line_idx,
                    col: start,
                    text: new_chars.clone(),
                });
            }
        }

        if changes.is_empty() {
            self.set_status(format!("E486: Pattern not found: {old}"));
            return Effect::None;
        }

        for change in &changes {
            self.apply(change);
        }
        self.clamp_cursor();
        self.commit(before, changes);
        self.highlight_active = false;
        if all_lines {
            self.set_status(format!("{hits} substitution(s) on {hit_lines} line(s)"));
        }
        Effect::None
    }

    fn save_cmd(&mut self, arg: Option<String>) -> Effect {
        let path: PathBuf = match arg {
            Some(a) => {
                let p = PathBuf::from(a);
                self.file_path = Some(p.clone());
                self.lang = Lang::from_path(&p);
                p
            }
            None => match &self.file_path {
                Some(p) => p.clone(),
                None => {
                    self.set_status(String::from("No file name: use :w <filename>"));
                    return Effect::None;
                }
            },
        };
        match self.save_to(&path) {
            Ok(()) => {
                self.set_status(format!(
                    "Wrote {} lines to {}",
                    self.lines.len(),
                    path.display()
                ));
                Effect::Saved(path)
            }
            Err(e) => {
                self.set_status(format!("Error saving {}: {e}", path.display()));
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
        self.saved_undo_len = self.undo_stack.len();
        self.coalesce_barrier = true;
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
        let before = (self.cx, self.cy);
        let old_content = self.lines[self.cy].clone();
        if self.lines.len() > 1 {
            let index = self.cy;
            self.lines.remove(index);
            self.cy = self.cy.min(self.lines.len() - 1);
            self.cx = 0;
            self.commit(
                before,
                vec![Change::LineDelete {
                    index,
                    line: old_content.clone(),
                }],
            );
        } else {
            self.lines[0].clear();
            self.cy = 0;
            self.cx = 0;
            self.commit(
                before,
                vec![Change::Delete {
                    line: 0,
                    col: 0,
                    text: old_content.clone(),
                }],
            );
        }
        self.register = Some(Register::Lines(vec![old_content]));
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
    "HELP: i insert | dd delete | yy copy | p paste | u undo | / search | :w save | :q quit | :q! force";

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

// ----------------------------------------------------------------------
// Base64 (standard alphabet) for OSC 52 payloads — hand-rolled to keep
// the dependency list at three crates.
// ----------------------------------------------------------------------

pub fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
