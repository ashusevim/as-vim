//! Integration tests for the editor core. No terminal required — the editor
//! module is pure logic.

use std::path::Path;

use as_vim::editor::{display_col, split_lines, Editor, Effect, Input, Mode};

fn ed() -> Editor {
    Editor::new(None, 24, 80)
}

fn ed_with(content: &str) -> Editor {
    Editor::from_content(content, None, 24, 80)
}

/// Drive the editor with a sequence of inputs, asserting none of them quits.
fn feed(ed: &mut Editor, inputs: &[Input]) {
    for &i in inputs {
        assert_ne!(ed.handle_input(i), Effect::Quit);
    }
}

fn text(ed: &Editor) -> String {
    ed.lines
        .iter()
        .map(|l| l.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

// ----------------------------------------------------------------------
// Buffer loading
// ----------------------------------------------------------------------

#[test]
fn empty_buffer_is_one_empty_line() {
    let e = ed();
    assert_eq!(e.lines, vec![Vec::<char>::new()]);
    assert_eq!(e.mode, Mode::Normal);
}

#[test]
fn split_lines_handles_trailing_newline_and_crlf() {
    assert_eq!(split_lines("a\nb\n").len(), 2);
    assert_eq!(split_lines("").len(), 1);
    assert_eq!(split_lines("\n").len(), 1);
    let crlf = split_lines("a\r\nb\r\n");
    let lines: Vec<String> = crlf.iter().map(|l| l.iter().collect::<String>()).collect();
    assert_eq!(lines, vec!["a", "b"]);
}

#[test]
fn open_missing_file_starts_new_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("missing.txt");
    let e = Editor::open(&p, 24, 80).unwrap();
    assert_eq!(e.lines.len(), 1);
    assert!(e.status.contains("New file"));
}

#[test]
fn open_existing_file_loads_content() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("f.txt");
    std::fs::write(&p, "hello\nworld").unwrap();
    let e = Editor::open(&p, 24, 80).unwrap();
    assert_eq!(text(&e), "hello\nworld");
}

#[test]
fn open_rejects_io_errors_other_than_not_found() {
    // Opening a directory as a file must fail, not silently load.
    let dir = tempfile::tempdir().unwrap();
    assert!(Editor::open(dir.path(), 24, 80).is_err());
}

// ----------------------------------------------------------------------
// INSERT mode
// ----------------------------------------------------------------------

#[test]
fn insert_types_characters_at_cursor() {
    let mut e = ed();
    feed(&mut e, &[Input::Char('i')]);
    for c in "abc".chars() {
        e.handle_input(Input::Char(c));
    }
    assert_eq!(text(&e), "abc");
    assert_eq!(e.cx, 3);
    assert!(e.dirty);
}

#[test]
fn insert_mid_line_splits_at_cursor() {
    let mut e = ed_with("hello");
    feed(
        &mut e,
        &[Input::Char('l'), Input::Char('l'), Input::Char('i')],
    );
    e.handle_input(Input::Char('X'));
    e.handle_input(Input::Enter);
    assert_eq!(text(&e), "heX\nllo");
    assert_eq!((e.cy, e.cx), (1, 0));
}

#[test]
fn backspace_removes_before_cursor_and_joins_lines() {
    let mut e = ed_with("ab\ncd");
    // move to start of line 2, press i, backspace -> joins to "abcd"
    feed(
        &mut e,
        &[Input::Char('j'), Input::Char('i'), Input::Backspace],
    );
    assert_eq!(text(&e), "abcd");
    assert_eq!((e.cy, e.cx), (0, 2));
}

#[test]
fn backspace_at_buffer_start_is_noop() {
    let mut e = ed_with("abc");
    feed(&mut e, &[Input::Char('i'), Input::Backspace]);
    assert_eq!(text(&e), "abc");
}

#[test]
fn delete_key_removes_char_at_cursor_or_joins_next() {
    let mut e = ed_with("abc");
    e.handle_input(Input::Delete);
    assert_eq!(text(&e), "bc");
    // Insert-mode Delete at end of an empty line joins the next line
    let mut e2 = ed_with("ab\n\ncd");
    feed(
        &mut e2,
        &[Input::Char('j'), Input::Char('i'), Input::Delete],
    );
    assert_eq!(text(&e2), "ab\ncd");
}

#[test]
fn escape_from_insert_clamps_cursor_onto_last_char() {
    let mut e = ed_with("abc");
    feed(
        &mut e,
        &[
            Input::Char('A'), // append mode -> cursor at end (past last char)
            Input::Esc,
        ],
    );
    assert_eq!(e.mode, Mode::Normal);
    assert_eq!(e.cx, 2); // sits on 'c'
}

#[test]
fn unicode_characters_insert_and_delete_as_units() {
    let mut e = ed();
    feed(&mut e, &[Input::Char('i')]);
    for c in "héllo🎉".chars() {
        e.handle_input(Input::Char(c));
    }
    assert_eq!(e.cx, "héllo🎉".chars().count());
    e.handle_input(Input::Backspace);
    assert_eq!(text(&e), "héllo");
    // cursor clamped back onto a char boundary
    e.handle_input(Input::Esc);
    assert_eq!(e.cx, "héllo".chars().count() - 1);
}

#[test]
fn tab_inserts_tab_character() {
    let mut e = ed();
    feed(&mut e, &[Input::Char('i'), Input::Tab, Input::Char('x')]);
    assert_eq!(text(&e), "\tx");
}

// ----------------------------------------------------------------------
// NORMAL mode
// ----------------------------------------------------------------------

#[test]
fn hjkl_respect_buffer_bounds() {
    let mut e = ed_with("ab\nabc");
    // l at end of line 1 stops on last char
    for _ in 0..10 {
        e.handle_input(Input::Char('l'));
    }
    assert_eq!(e.cx, 1); // "ab" -> index 1
                         // k at top does nothing
    e.handle_input(Input::Char('k'));
    assert_eq!(e.cy, 0);
    // h moves left, then stops at col 0
    e.handle_input(Input::Char('h'));
    assert_eq!(e.cx, 0);
    e.handle_input(Input::Char('h'));
    assert_eq!(e.cx, 0);
    // j at bottom of 2-line buffer stops at line 1
    e.handle_input(Input::Char('j'));
    e.handle_input(Input::Char('j'));
    assert_eq!(e.cy, 1);
}

#[test]
fn j_clamps_column_to_shorter_line() {
    let mut e = ed_with("abcdef\nxy");
    feed(&mut e, &[Input::Char('$'), Input::Char('j')]);
    assert_eq!((e.cy, e.cx), (1, 1)); // line 2 is "xy", max col 1
}

#[test]
fn zero_dollar_g_and_g_motions() {
    let mut e = ed_with("ab\ncd\nef");
    e.handle_input(Input::Char('G'));
    assert_eq!(e.cy, 2);
    e.handle_input(Input::Char('g'));
    assert_eq!(e.cy, 0);
    e.handle_input(Input::Char('$'));
    assert_eq!(e.cx, 1);
    e.handle_input(Input::Char('0'));
    assert_eq!(e.cx, 0);
}

#[test]
fn x_deletes_char_at_cursor() {
    let mut e = ed_with("abc");
    e.handle_input(Input::Char('x'));
    assert_eq!(text(&e), "bc");
    assert!(e.dirty);
}

#[test]
fn dd_deletes_line_and_keeps_one_line_buffer() {
    let mut e = ed_with("a\nb\nc");
    e.handle_input(Input::Char('d'));
    e.handle_input(Input::Char('d'));
    assert_eq!(text(&e), "b\nc"); // deleted current line (0)
                                  // reduce to a single line, then dd empties it instead
    e.handle_input(Input::Char('d'));
    e.handle_input(Input::Char('d'));
    assert_eq!(text(&e), "c");
    e.handle_input(Input::Char('d'));
    e.handle_input(Input::Char('d'));
    assert_eq!(text(&e), "");
    assert_eq!(e.lines.len(), 1);
}

#[test]
fn pending_d_is_cancelled_by_other_keys() {
    let mut e = ed_with("a\nb");
    e.handle_input(Input::Char('d'));
    e.handle_input(Input::Char('h')); // not dd
    assert_eq!(text(&e), "a\nb");
    e.handle_input(Input::Char('d'));
    e.handle_input(Input::Char('d'));
    assert_eq!(text(&e), "b"); // dd deletes the line the cursor is on
}

#[test]
#[allow(non_snake_case)]
fn o_and_O_open_lines_and_enter_insert() {
    let mut e = ed_with("ab");
    e.handle_input(Input::Char('o'));
    assert_eq!((e.mode, e.cy), (Mode::Insert, 1));
    e.handle_input(Input::Char('z'));
    assert_eq!(text(&e), "ab\nz");
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('O'));
    e.handle_input(Input::Char('q'));
    assert_eq!(text(&e), "ab\nq\nz");
}

#[test]
#[allow(non_snake_case)]
fn a_and_A_enter_insert_at_right_positions() {
    let mut e = ed_with("abc");
    e.handle_input(Input::Char('a'));
    assert_eq!(e.cx, 1);
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('A'));
    assert_eq!(e.cx, 3);
    e.handle_input(Input::Char('!'));
    assert_eq!(text(&e), "abc!");
}

#[test]
fn enter_in_normal_moves_to_next_line_col0() {
    let mut e = ed_with("abc\ndef");
    e.handle_input(Input::Enter);
    assert_eq!((e.cy, e.cx), (1, 0));
}

#[test]
fn page_up_down_scroll_by_page() {
    let content: String = (0..100).map(|i| format!("line{i}\n")).collect();
    let mut e = ed_with(&content);
    e.handle_input(Input::Char('G'));
    assert_eq!(e.cy, 99);
    e.handle_input(Input::PageUp);
    // page height = 24 - 2 = 22
    assert_eq!(e.cy, 77);
    e.handle_input(Input::PageDown);
    assert_eq!(e.cy, 99);
}

#[test]
fn ctrl_c_in_normal_hints_at_quit() {
    let mut e = ed();
    e.handle_input(Input::Ctrl('c'));
    assert!(e.status.contains(":q"));
    assert_ne!(e.handle_input(Input::Ctrl('c')), Effect::Quit);
}

// ----------------------------------------------------------------------
// COMMAND mode
// ----------------------------------------------------------------------

#[test]
fn colon_enters_command_and_esc_cancels() {
    let mut e = ed();
    e.handle_input(Input::Char(':'));
    assert_eq!(e.mode, Mode::Command);
    e.handle_input(Input::Char('w'));
    assert_eq!(e.command, "w");
    e.handle_input(Input::Esc);
    assert_eq!(e.mode, Mode::Normal);
    assert!(e.command.is_empty());
}

#[test]
fn unknown_command_sets_error_message() {
    let mut e = ed();
    feed(&mut e, &[Input::Char(':'), Input::Char('z')]);
    assert_eq!(e.handle_input(Input::Enter), Effect::None);
    assert!(e.status.contains("Not an editor command"));
}

#[test]
fn bare_colon_enter_is_noop() {
    let mut e = ed();
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Enter);
    assert_eq!(e.mode, Mode::Normal);
}

#[test]
fn q_on_clean_buffer_quits() {
    let mut e = ed();
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Char('q'));
    assert_eq!(e.handle_input(Input::Enter), Effect::Quit);
}

#[test]
fn q_with_unsaved_changes_warns_instead_of_quitting() {
    let mut e = ed_with("dirty");
    e.handle_input(Input::Char('x')); // make dirty
    e.handle_input(Input::Char(':'));
    for c in "q".chars() {
        e.handle_input(Input::Char(c));
    }
    assert_eq!(e.handle_input(Input::Enter), Effect::None);
    assert!(e.status.contains("No write since last change"));
    // :q! still quits
    e.handle_input(Input::Char(':'));
    for c in "q!".chars() {
        e.handle_input(Input::Char(c));
    }
    assert_eq!(e.handle_input(Input::Enter), Effect::Quit);
}

#[test]
fn wq_saves_then_quits() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("wq.txt");
    let mut e = Editor::open(&p, 24, 80).unwrap();
    feed(&mut e, &[Input::Char('i')]);
    for c in "saved".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char(':'));
    for c in "wq".chars() {
        e.handle_input(Input::Char(c));
    }
    assert_eq!(e.handle_input(Input::Enter), Effect::Quit);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "saved\n");
    assert!(!e.dirty);
}

#[test]
fn w_writes_buffer_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("w.txt");
    let mut e = Editor::open(&p, 24, 80).unwrap();
    feed(&mut e, &[Input::Char('i')]);
    for c in "one\ntwo".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Char('w'));
    let effect = e.handle_input(Input::Enter);
    assert!(matches!(effect, Effect::Saved(_)));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\ntwo\n");
    assert!(!e.dirty);
}

#[test]
fn w_with_filename_saves_to_new_path() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("renamed.txt");
    let mut e = ed();
    e.handle_input(Input::Char(':'));
    for c in format!("w {}", p.display()).chars() {
        e.handle_input(Input::Char(c));
    }
    let effect = e.handle_input(Input::Enter);
    assert!(matches!(effect, Effect::Saved(_)));
    assert_eq!(e.file_path.as_deref(), Some(Path::new(&p)));
    assert!(std::fs::read_to_string(&p).unwrap().ends_with('\n'));
}

#[test]
fn w_without_filename_prompts_for_one() {
    let mut e = ed();
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Char('w'));
    e.handle_input(Input::Enter);
    assert!(e.status.contains("No file name"));
}

#[test]
fn saving_clears_dirty_flag() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("d.txt");
    let mut e = Editor::open(&p, 24, 80).unwrap();
    feed(&mut e, &[Input::Char('i')]);
    e.handle_input(Input::Char('t'));
    e.handle_input(Input::Esc);
    assert!(e.dirty);
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Char('w'));
    e.handle_input(Input::Enter);
    assert!(!e.dirty);
    // :q now quits cleanly
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Char('q'));
    assert_eq!(e.handle_input(Input::Enter), Effect::Quit);
}

// ----------------------------------------------------------------------
// Scrolling + display widths
// ----------------------------------------------------------------------

#[test]
fn update_scroll_keeps_cursor_visible_vertically() {
    let content: String = (0..100).map(|i| format!("{i}\n")).collect();
    let mut e = ed_with(&content);
    e.handle_input(Input::Char('G')); // line 100, page height 22
    assert_eq!(e.cy, 99);
    e.update_scroll();
    assert_eq!(e.row_off, 99 + 1 - 22);
    e.cy = 0;
    e.update_scroll();
    assert_eq!(e.row_off, 0);
}

#[test]
fn display_col_counts_tabs_as_tab_width() {
    let line: Vec<char> = "\tx".chars().collect();
    assert_eq!(display_col(&line, 0), 0);
    assert_eq!(display_col(&line, 1), 4);
    assert_eq!(display_col(&line, 2), 5);
}

#[test]
fn wide_unicode_occupies_two_columns() {
    // CJK characters are double-width
    let line: Vec<char> = "日本".chars().collect();
    assert_eq!(display_col(&line, 1), 2);
    assert_eq!(display_col(&line, 2), 4);
}

#[test]
fn horizontal_scroll_follows_cursor() {
    let mut e = ed_with(&"a".repeat(200));
    e.handle_input(Input::Char('$'));
    e.update_scroll();
    let col = display_col(&e.lines[0], e.cx);
    assert!(col >= e.col_off && col < e.col_off + e.screen_cols);
}
