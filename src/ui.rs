//! Renders [`Editor`] state to the terminal via raw ANSI through crossterm.
//!
//! Layout (same as the original as-nano):
//! - rows `0 .. screen_rows-2`: text area (`~` past EOF)
//! - row `screen_rows-2`: inverted status bar (or `:command` prompt)
//! - row `screen_rows-1`: status message line

use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};

use crate::editor::{display_col, Editor, Mode};

pub fn refresh_screen(w: &mut impl Write, ed: &mut Editor) -> io::Result<()> {
    ed.update_scroll();

    queue!(w, Hide)?;
    queue!(w, Clear(ClearType::All))?;

    let text_rows = ed.screen_rows.saturating_sub(2);
    for row in 0..text_rows {
        queue!(w, MoveTo(0, row as u16), Clear(ClearType::CurrentLine))?;
        let buf_row = ed.row_off + row;
        if buf_row < ed.lines.len() {
            let line = &ed.lines[buf_row];
            let visible = visible_slice(line, ed.col_off, ed.screen_cols);
            queue!(w, Print(visible))?;
        } else {
            queue!(
                w,
                SetForegroundColor(Color::DarkGrey),
                Print("~"),
                ResetColor
            )?;
        }
    }

    // Status bar.
    let status_row = ed.screen_rows.saturating_sub(2);
    let bar = status_bar_text(ed);
    queue!(
        w,
        MoveTo(0, status_row as u16),
        Clear(ClearType::CurrentLine),
        SetAttribute(Attribute::Reverse),
        Print(truncate(&bar, ed.screen_cols)),
        SetAttribute(Attribute::NoReverse),
        ResetColor
    )?;

    // Message line.
    let message_row = ed.screen_rows.saturating_sub(1);
    queue!(
        w,
        MoveTo(0, message_row as u16),
        Clear(ClearType::CurrentLine),
        Print(truncate(&ed.status, ed.screen_cols))
    )?;

    // Cursor position.
    let (col, row) = cursor_pos(ed);
    queue!(w, MoveTo(col as u16, row as u16), Show)?;
    w.flush()
}

fn status_bar_text(ed: &Editor) -> String {
    if ed.mode == Mode::Command {
        return format!(":{}", ed.command);
    }
    let position = format!("{}:{}", ed.cy + 1, ed.cx + 1);
    let file = ed
        .file_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| String::from("[No Name]"));
    let dirty = if ed.dirty { " [+]" } else { "" };
    format!(
        "-- {} -- | {} | {}{}",
        ed.mode.label(),
        position,
        file,
        dirty
    )
}

/// Screen position (col, row) for the terminal cursor.
fn cursor_pos(ed: &Editor) -> (usize, usize) {
    if ed.mode == Mode::Command {
        let status_row = ed.screen_rows.saturating_sub(2);
        return (ed.command.chars().count() + 1, status_row);
    }
    let text_rows = ed.screen_rows.saturating_sub(2).max(1);
    let row = (ed.cy - ed.row_off).min(text_rows - 1);
    let col = display_col(&ed.lines[ed.cy], ed.cx)
        .saturating_sub(ed.col_off)
        .min(ed.screen_cols.saturating_sub(1));
    (col, row)
}

/// Render the buffer line starting at display column `col_off`, producing at
/// most `max_cols` display columns. Tabs expand to `TAB_WIDTH` spaces.
fn visible_slice(line: &[char], col_off: usize, max_cols: usize) -> String {
    use crate::editor::char_display_width;

    let mut out = String::new();
    let mut skipped = 0;
    let mut width = 0;
    for &c in line {
        let cw = char_display_width(c);
        if skipped + cw <= col_off {
            skipped += cw;
            continue;
        }
        if width + cw > max_cols {
            break;
        }
        if c == '\t' {
            out.push_str(&" ".repeat(crate::editor::TAB_WIDTH));
        } else {
            out.push(c);
        }
        width += cw;
    }
    out
}

/// Truncate a string to roughly `max_cols` display columns.
fn truncate(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    let mut out = String::new();
    let mut width = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(1);
        if width + cw > max_cols {
            break;
        }
        out.push(c);
        width += cw;
    }
    out
}
