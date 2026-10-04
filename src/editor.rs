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

    /// True when the buffer differs from the last saved state. Derived from
    /// the undo stack, so undoing back to the saved state also clears it.
    pub fn dirty(&self) -> bool {
        self.undo_stack.len() != self.saved_undo_len
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
                self.status = String::from("-- INSERT -- (Esc for NORMAL)");
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
                self.status = String::from("-- INSERT -- (Esc for NORMAL)");
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
            self.status = String::from("Already at oldest change");
            return;
        };
        for change in tx.changes.iter().rev() {
            self.revert(change);
        }
        (self.cx, self.cy) = tx.before;
        self.redo_stack.push(tx.clone());
        self.status = format!("Undid {} change(s)", tx.changes.len());
    }

    pub fn redo(&mut self) {
        let Some(tx) = self.redo_stack.pop() else {
            self.status = String::from("Already at newest change");
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
        self.status = format!("Redid {} change(s)", tx.changes.len());
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
        self.status = format!("Yanked line {} (also copied via OSC 52)", self.cy + 1);
    }

    fn paste(&mut self, before: bool) {
        let reg = match &self.register {
            Some(r) => r.clone(),
            None => {
                self.status = String::from("Nothing to paste");
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
                self.status = format!("Pasted {n} line(s)");
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
                self.status = format!("Pasted {n} char(s)");
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
            self.status = String::from("No previous search");
            return;
        };
        if term.is_empty() {
            return;
        }
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
                self.status = format!("/{}", term);
            }
            None => {
                self.status = format!("Pattern not found: {}", term);
            }
        }
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
                if self.dirty() {
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
            ("undo", _) => {
                self.undo();
                Effect::None
            }
            ("redo", _) => {
                self.redo();
                Effect::None
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
                self.lang = Lang::from_path(&p);
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
