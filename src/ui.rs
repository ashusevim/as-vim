//! Renders [`Editor`] state to the terminal via raw ANSI through crossterm.
//!
//! Layout (same as the original as-nano):
//! - rows `0 .. screen_rows-2`: text area (`~` past EOF), `/search` matches
//!   highlighted
//! - row `screen_rows-2`: inverted status bar (`:command` or `/search` prompt)
//! - row `screen_rows-1`: status message line

use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};

use crate::editor::{char_display_width, display_col, Editor, Mode, TAB_WIDTH};
use crate::syntax::{highlight_line, Class, State as SyntaxState};

/// Foreground color per syntax class; None = terminal default.
fn color_for(class: Class) -> Option<Color> {
    match class {
        Class::Normal => None,
        Class::Keyword => Some(Color::Blue),
        Class::Type => Some(Color::Cyan),
        Class::Str => Some(Color::Green),
        Class::Comment => Some(Color::DarkGrey),
        Class::Number => Some(Color::Magenta),
    }
}

pub fn refresh_screen(w: &mut impl Write, ed: &mut Editor) -> io::Result<()> {
    ed.update_scroll();

    queue!(w, Hide)?;
    queue!(w, Clear(ClearType::All))?;

    let text_rows = ed.screen_rows.saturating_sub(2);

    // Syntax state at the first visible line: replay the highlighter over
    // the scrolled-past lines (block comments are the only cross-line state).
    let mut syntax_state = SyntaxState::Normal;
    if let Some(lang) = ed.lang {
        for line in ed.lines.iter().take(ed.row_off) {
            syntax_state = highlight_line(lang, line, syntax_state).1;
        }
    }

    for row in 0..text_rows {
        queue!(w, MoveTo(0, row as u16), Clear(ClearType::CurrentLine))?;
        let buf_row = ed.row_off + row;
        if buf_row < ed.lines.len() {
            let (classes, next) = match ed.lang {
                Some(lang) => highlight_line(lang, &ed.lines[buf_row], syntax_state),
                None => (vec![Class::Normal; ed.lines[buf_row].len()], syntax_state),
            };
            syntax_state = next;
            let ranges = ed.search_ranges(buf_row);
            draw_styled(
                w,
                &ed.lines[buf_row],
                &classes,
                &ranges,
                ed.col_off,
                ed.screen_cols,
            )?;
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
    if ed.mode == Mode::Search {
        return format!("/{}", ed.search_term);
    }
    let position = format!("{}:{}", ed.cy + 1, ed.cx + 1);
    let file = ed
        .file_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| String::from("[No Name]"));
    let dirty = if ed.dirty() { " [+]" } else { "" };
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
    if ed.mode == Mode::Search {
        let status_row = ed.screen_rows.saturating_sub(2);
        return (ed.search_term.chars().count() + 1, status_row);
    }
    let text_rows = ed.screen_rows.saturating_sub(2).max(1);
    let row = (ed.cy - ed.row_off).min(text_rows - 1);
    let col = display_col(&ed.lines[ed.cy], ed.cx)
        .saturating_sub(ed.col_off)
        .min(ed.screen_cols.saturating_sub(1));
    (col, row)
}

/// Render a buffer line with per-character foreground colors (syntax) and
/// background highlights (search matches), honouring horizontal scroll and
/// tab expansion. Consecutive chars with identical styling are printed as
/// one run.
fn draw_styled(
    w: &mut impl Write,
    line: &[char],
    classes: &[Class],
    ranges: &[(usize, usize)],
    col_off: usize,
    max_cols: usize,
) -> io::Result<()> {
    let mut group = String::new();
    let mut group_style: (Option<Color>, bool) = (None, false);
    let mut skipped = 0usize;
    let mut width = 0usize;

    for (idx, &c) in line.iter().enumerate() {
        let cw = char_display_width(c);
        if skipped + cw <= col_off {
            skipped += cw;
            continue;
        }
        if width + cw > max_cols {
            break;
        }
        let fg = classes.get(idx).copied().unwrap_or(Class::Normal);
        let style = (
            color_for(fg),
            ranges.iter().any(|r| idx >= r.0 && idx < r.1),
        );
        if style != group_style && !group.is_empty() {
            flush_group(w, &group, group_style)?;
            group.clear();
        }
        group_style = style;
        if c == '\t' {
            group.push_str(&" ".repeat(TAB_WIDTH));
        } else {
            group.push(c);
        }
        width += cw;
    }
    flush_group(w, &group, group_style)
}

fn flush_group(w: &mut impl Write, group: &str, style: (Option<Color>, bool)) -> io::Result<()> {
    if group.is_empty() {
        return Ok(());
    }
    let (fg, highlight) = style;
    if let Some(color) = fg {
        queue!(w, SetForegroundColor(color))?;
    }
    if highlight {
        queue!(w, SetBackgroundColor(Color::DarkGrey))?;
    }
    queue!(w, Print(group))?;
    if highlight || fg.is_some() {
        queue!(w, ResetColor)?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Lang;

    fn render(line: &str, ranges: &[(usize, usize)]) -> Vec<u8> {
        let lang = Lang::Rust;
        let chars: Vec<char> = line.chars().collect();
        let (classes, _) = highlight_line(lang, &chars, SyntaxState::Normal);
        let mut buf = Vec::new();
        draw_styled(&mut buf, &chars, &classes, ranges, 0, 80).unwrap();
        buf
    }

    #[test]
    fn keyword_gets_color_escape() {
        let buf = render("let x = 1;", &[]);
        let s = String::from_utf8(buf).unwrap();
        // crossterm emits 256-color codes: Blue = 38;5;12
        assert!(s.contains("\x1b[38;5;12mlet"), "got: {s:?}");
    }

    #[test]
    fn search_match_gets_background() {
        let buf = render("let x", &[(0, 3)]);
        let s = String::from_utf8(buf).unwrap();
        // DarkGrey background = 48;5;8
        assert!(s.contains("\x1b[48;5;8m"), "bg missing: {s:?}");
    }

    #[test]
    fn plain_chars_have_no_color() {
        let buf = render("xy", &[]);
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "xy"); // no escapes at all for Normal-only runs
    }
}
