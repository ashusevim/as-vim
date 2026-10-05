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
    assert!(e.dirty());
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
    assert!(e.dirty());
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
    assert!(!e.dirty());
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
    assert!(!e.dirty());
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
    assert!(e.dirty());
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Char('w'));
    e.handle_input(Input::Enter);
    assert!(!e.dirty());
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

// ----------------------------------------------------------------------
// v0.2 — Undo / redo
// ----------------------------------------------------------------------

#[test]
fn undo_removes_last_insert() {
    let mut e = ed();
    feed(&mut e, &[Input::Char('i')]);
    for c in "hello".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "");
    assert!(!e.dirty());
}

#[test]
fn consecutive_typing_coalesces_into_one_undo() {
    let mut e = ed();
    feed(&mut e, &[Input::Char('i')]);
    for c in "abc".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), ""); // one undo clears all of "abc"
}

#[test]
fn cursor_movement_breaks_insert_coalescing() {
    let mut e = ed();
    feed(&mut e, &[Input::Char('i')]);
    for c in "ab".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::ArrowLeft); // move between a and b
    e.handle_input(Input::Char('X'));
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "ab"); // only "X" undone
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "");
}

#[test]
fn undo_and_redo_enter_split_roundtrip() {
    let mut e = ed_with("hello");
    feed(
        &mut e,
        &[Input::Char('l'), Input::Char('l'), Input::Char('i')],
    );
    e.handle_input(Input::Enter);
    assert_eq!(text(&e), "he\nllo");
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "hello");
    e.handle_input(Input::Ctrl('r'));
    assert_eq!(text(&e), "he\nllo");
}

#[test]
fn undo_backspace_join_restores_the_line() {
    let mut e = ed_with("ab\ncd");
    feed(
        &mut e,
        &[Input::Char('j'), Input::Char('i'), Input::Backspace],
    );
    assert_eq!(text(&e), "abcd");
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "ab\ncd");
}

#[test]
fn undo_dd_restores_deleted_line() {
    let mut e = ed_with("a\nb\nc");
    feed(&mut e, &[Input::Char('d'), Input::Char('d')]);
    assert_eq!(text(&e), "b\nc");
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "a\nb\nc");
    assert_eq!((e.cy, e.cx), (0, 0)); // cursor restored to before the dd
}

#[test]
#[allow(non_snake_case)]
fn undo_o_and_O() {
    let mut e = ed_with("mid");
    e.handle_input(Input::Char('o'));
    e.handle_input(Input::Char('z'));
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "mid");
    // redo: reopens the line, cursor at its start (vim behaviour)
    e.handle_input(Input::Ctrl('r'));
    assert_eq!(text(&e), "mid\nz");
    assert_eq!((e.cy, e.cx), (1, 0));

    let mut e2 = ed_with("mid");
    e2.handle_input(Input::Char('O'));
    e2.handle_input(Input::Char('q'));
    e2.handle_input(Input::Esc);
    e2.handle_input(Input::Char('u'));
    assert_eq!(text(&e2), "mid");
    e2.handle_input(Input::Ctrl('r'));
    assert_eq!(text(&e2), "q\nmid");
    assert_eq!(e2.cy, 0);
}

#[test]
fn undo_x_restores_char() {
    let mut e = ed_with("abc");
    e.handle_input(Input::Char('x'));
    assert_eq!(text(&e), "bc");
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "abc");
}

#[test]
fn redo_is_cleared_by_a_new_edit() {
    let mut e = ed_with("a");
    feed(&mut e, &[Input::Char('i'), Input::Char('b')]); // "ba"
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "a");
    feed(&mut e, &[Input::Char('i'), Input::Char('c')]); // "ca"
    e.handle_input(Input::Esc);
    // redo must do nothing now — the edit after undo cleared it
    e.handle_input(Input::Ctrl('r'));
    assert_eq!(text(&e), "ca");
    assert!(e.status.contains("newest") || e.status.contains("Redid"));
}

#[test]
fn undo_at_start_and_redo_at_end_report_status() {
    let mut e = ed_with("a");
    e.handle_input(Input::Char('u'));
    assert!(e.status.contains("oldest"));
    e.handle_input(Input::Ctrl('r'));
    assert!(e.status.contains("newest"));
}

#[test]
fn undoing_back_to_saved_state_clears_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.txt");
    let mut e = Editor::open(&p, 24, 80).unwrap();
    feed(&mut e, &[Input::Char('i'), Input::Char('x')]);
    e.handle_input(Input::Esc);
    assert!(e.dirty());
    e.handle_input(Input::Char(':'));
    e.handle_input(Input::Char('w'));
    e.handle_input(Input::Enter);
    assert!(!e.dirty()); // just saved
    feed(&mut e, &[Input::Char('i'), Input::Char('y')]);
    e.handle_input(Input::Esc);
    assert!(e.dirty());
    // continue typing after the save — still dirty (y+z coalesce into one tx)
    feed(&mut e, &[Input::Char('i'), Input::Char('z')]);
    e.handle_input(Input::Esc);
    assert!(e.dirty());
    e.handle_input(Input::Char('u')); // one undo removes the y+z session
    assert!(!e.dirty()); // exactly back to the saved state
    assert_eq!(text(&e), "x");
    e.handle_input(Input::Char('u')); // past the saved state -> dirty again
    assert!(e.dirty());
    assert_eq!(text(&e), "");
    e.handle_input(Input::Ctrl('r')); // redo returns to the saved state
    assert!(!e.dirty());
    assert_eq!(text(&e), "x");
}

#[test]
fn undo_and_redo_commands_work() {
    let mut e = ed_with("a");
    feed(&mut e, &[Input::Char('i'), Input::Char('b')]);
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char(':'));
    for c in "undo".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter);
    assert_eq!(text(&e), "a");
    e.handle_input(Input::Char(':'));
    for c in "redo".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter);
    assert_eq!(text(&e), "ba");
}

// ----------------------------------------------------------------------
// v0.2 — Search
// ----------------------------------------------------------------------

#[test]
fn search_jumps_to_next_match_and_wraps() {
    let mut e = ed_with("one two\nthree two\nfour");
    e.handle_input(Input::Char('/'));
    assert_eq!(e.mode, Mode::Search);
    for c in "two".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter);
    assert_eq!((e.cy, e.cx), (0, 4));
    e.handle_input(Input::Char('n')); // next: line 1
    assert_eq!((e.cy, e.cx), (1, 6));
    e.handle_input(Input::Char('n')); // wraps to line 0
    assert_eq!((e.cy, e.cx), (0, 4));
}

#[test]
#[allow(non_snake_case)]
fn capital_N_searches_backwards() {
    let mut e = ed_with("aa\naa");
    feed(&mut e, &[Input::Char('j'), Input::Char('l')]);
    e.handle_input(Input::Char('/'));
    for c in "aa".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter); // from (1,1): next "aa" wraps to (0,0)
    assert_eq!((e.cy, e.cx), (0, 0));
    e.handle_input(Input::Char('N')); // backwards: (1,0)
    assert_eq!((e.cy, e.cx), (1, 0));
}

#[test]
fn search_esc_cancels_but_keeps_last_search_for_n() {
    let mut e = ed_with("abc abc");
    e.handle_input(Input::Char('/'));
    for c in "abc".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Esc);
    assert_eq!(e.mode, Mode::Normal);
    // last search was never executed; n reports no previous search
    e.handle_input(Input::Char('n'));
    assert!(e.status.contains("No previous search"));
}

#[test]
fn search_no_match_reports_not_found() {
    let mut e = ed_with("hello");
    e.handle_input(Input::Char('/'));
    for c in "zzz".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter);
    assert!(e.status.contains("Pattern not found"));
    assert_eq!((e.cy, e.cx), (0, 0));
}

#[test]
fn search_is_smartcase() {
    // all-lowercase term: case-insensitive
    assert_eq!(
        Editor::find_matches(&"Hello HELLO".chars().collect::<Vec<_>>(), "hello"),
        vec![0, 6]
    );
    // term with uppercase: case-sensitive
    assert_eq!(
        Editor::find_matches(&"Hello HELLO".chars().collect::<Vec<_>>(), "HELLO"),
        vec![6]
    );
}

#[test]
fn find_matches_handles_empty_and_short() {
    assert!(Editor::find_matches(&"abc".chars().collect::<Vec<_>>(), "").is_empty());
    assert!(Editor::find_matches(&"a".chars().collect::<Vec<_>>(), "abc").is_empty());
    assert_eq!(
        Editor::find_matches(&"aaa".chars().collect::<Vec<_>>(), "aa"),
        vec![0]
    ); // non-overlapping
}

#[test]
fn search_ranges_feed_highlighting() {
    let mut e = ed_with("cat catalog");
    e.handle_input(Input::Char('/'));
    for c in "cat".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter);
    assert_eq!(e.search_ranges(0), vec![(0, 3), (4, 7)]);
    // untouched lines have no ranges
    e.lines.push("dog".chars().collect());
    assert!(e.search_ranges(1).is_empty());
}

#[test]
fn search_term_incremental_backspace() {
    let mut e = ed_with("target");
    e.handle_input(Input::Char('/'));
    for c in "tarx".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Backspace);
    assert_eq!(e.search_term, "tar");
    e.handle_input(Input::Enter);
    assert_eq!((e.cy, e.cx), (0, 0));
}

// ----------------------------------------------------------------------
// v0.2 — Yank / paste / OSC 52 clipboard
// ----------------------------------------------------------------------

#[test]
fn yy_p_pastes_line_below_and_is_undoable() {
    let mut e = ed_with("one\ntwo");
    feed(
        &mut e,
        &[
            Input::Char('j'),
            Input::Char('y'),
            Input::Char('y'),
            Input::Char('p'),
        ],
    );
    assert_eq!(text(&e), "one\ntwo\ntwo");
    assert_eq!(e.cy, 2);
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "one\ntwo");
}

#[test]
#[allow(non_snake_case)]
fn capital_P_pastes_line_above() {
    let mut e = ed_with("one\ntwo");
    feed(
        &mut e,
        &[
            Input::Char('j'),
            Input::Char('y'),
            Input::Char('y'),
            Input::Char('P'),
        ],
    );
    assert_eq!(text(&e), "one\ntwo\ntwo");
    assert_eq!(e.cy, 1); // pasted line takes position 1
}

#[test]
fn x_then_p_pastes_char_after_cursor() {
    let mut e = ed_with("abc");
    e.handle_input(Input::Char('x')); // deletes 'a', register = "a"
    e.handle_input(Input::Char('p')); // pastes after cursor char -> "bac"
    assert_eq!(text(&e), "bac");
    e.handle_input(Input::Esc);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "bc");
}

#[test]
fn dd_yanks_the_line_into_the_register() {
    let mut e = ed_with("kill\nkeep");
    e.handle_input(Input::Char('d'));
    e.handle_input(Input::Char('d'));
    assert_eq!(text(&e), "keep");
    e.handle_input(Input::Char('p'));
    assert_eq!(text(&e), "keep\nkill");
}

#[test]
fn paste_with_empty_register_is_a_noop_with_message() {
    let mut e = ed_with("abc");
    e.handle_input(Input::Char('p'));
    assert_eq!(text(&e), "abc");
    assert!(e.status.contains("Nothing to paste"));
}

#[test]
fn take_clipboard_returns_and_clears_pending() {
    let mut e = ed_with("data");
    e.handle_input(Input::Char('y'));
    e.handle_input(Input::Char('y'));
    let clip = e.take_clipboard();
    assert_eq!(clip.as_deref(), Some("data"));
    assert!(e.take_clipboard().is_none());
}

#[test]
fn base64_encode_matches_known_vectors() {
    use as_vim::editor::base64_encode;
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(
        base64_encode(b"any carnal pleasure."),
        "YW55IGNhcm5hbCBwbGVhc3VyZS4="
    );
}

#[test]
fn yank_clipboard_skips_huge_payloads() {
    let mut e = ed_with(&"x".repeat(150_000));
    e.handle_input(Input::Char('y'));
    e.handle_input(Input::Char('y'));
    assert!(e.take_clipboard().is_none()); // > 100 KB cap
}

// ----------------------------------------------------------------------
// v0.4 — :s / :%s substitute
// ----------------------------------------------------------------------

fn colon_cmd(e: &mut Editor, cmd: &str) -> Effect {
    e.handle_input(Input::Char(':'));
    for c in cmd.chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter)
}

#[test]
fn substitute_first_occurrence_on_current_line() {
    let mut e = ed_with("cat catalog cat");
    colon_cmd(&mut e, "s/cat/dog/");
    assert_eq!(text(&e), "dog catalog cat");
}

#[test]
fn substitute_global_flag() {
    let mut e = ed_with("cat catalog cat");
    colon_cmd(&mut e, "s/cat/dog/g");
    assert_eq!(text(&e), "dog dogalog dog");
}

#[test]
fn substitute_is_one_undo_step() {
    let mut e = ed_with("aa\naa");
    colon_cmd(&mut e, "%s/aa/bb/g");
    assert_eq!(text(&e), "bb\nbb");
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "aa\naa");
    e.handle_input(Input::Ctrl('r'));
    assert_eq!(text(&e), "bb\nbb");
}

#[test]
fn substitute_all_lines_percent() {
    let mut e = ed_with("x = 1\ny = 2\nx = 3");
    colon_cmd(&mut e, "%s/x/z/g");
    assert_eq!(text(&e), "z = 1\ny = 2\nz = 3");
}

#[test]
fn substitute_empty_replacement_deletes() {
    let mut e = ed_with("foo barfoo");
    colon_cmd(&mut e, "%s/foo//g");
    assert_eq!(text(&e), " bar");
}

#[test]
fn substitute_reuses_last_search_when_old_empty() {
    let mut e = ed_with("loop loop");
    feed(
        &mut e,
        &[Input::Char('/'), Input::Char('o'), Input::Char('p')],
    );
    e.handle_input(Input::Enter);
    // edit clears highlight but not last_search
    e.handle_input(Input::Char('x')); // "loop loop" -> "lop loop"
    colon_cmd(&mut e, "s//OP/g");
    assert_eq!(text(&e), "lOP loOP");
}

#[test]
fn substitute_smartcase() {
    let mut e = ed_with("Hello hello HELLO");
    colon_cmd(&mut e, "%s/hello/X/g"); // lowercase term: case-insensitive
    assert_eq!(text(&e), "X X X");
    let mut e2 = ed_with("Hello hello HELLO");
    colon_cmd(&mut e2, "%s/HELLO/X/g"); // uppercase term: case-sensitive
    assert_eq!(text(&e2), "Hello hello X");
}

#[test]
fn substitute_escaped_slash() {
    let mut e = ed_with("a/b c");
    colon_cmd(&mut e, "s/a\\/b/AB/");
    assert_eq!(text(&e), "AB c");
}

#[test]
fn substitute_no_match_reports_and_unchanged() {
    let mut e = ed_with("hello");
    colon_cmd(&mut e, "%s/zzz/q/g");
    assert_eq!(text(&e), "hello");
    assert!(e.status.contains("Pattern not found"));
}

#[test]
fn substitute_usage_errors() {
    let mut e = ed_with("x");
    colon_cmd(&mut e, "s/nope");
    assert!(e.status.contains("Usage"), "{}", e.status);
    let mut e2 = ed_with("x");
    colon_cmd(&mut e2, "s//y/"); // no previous search either
    assert!(e2.status.contains("No previous search"));
}

#[test]
fn substitute_reports_count_for_percent() {
    let mut e = ed_with("ab ab\nab");
    colon_cmd(&mut e, "%s/ab/X/g");
    assert!(
        e.status.contains("3 substitution(s) on 2 line(s)"),
        "{}",
        e.status
    );
}

#[test]
fn parse_substitute_cases() {
    use as_vim::editor::Editor;
    assert_eq!(
        Editor::parse_substitute("/a/b/").unwrap(),
        ("a".to_string(), "b".to_string(), false)
    );
    assert_eq!(
        Editor::parse_substitute("/a/b/g").unwrap(),
        ("a".to_string(), "b".to_string(), true)
    );
    assert_eq!(
        Editor::parse_substitute("/a/b").unwrap(),
        ("a".to_string(), "b".to_string(), false)
    );
    assert_eq!(
        Editor::parse_substitute("/a\\/b/c\\\\d/").unwrap(),
        ("a/b".to_string(), "c\\d".to_string(), false)
    );
    assert!(Editor::parse_substitute("a/b/").is_err());
    assert!(Editor::parse_substitute("/a").is_err());
}

// ----------------------------------------------------------------------
// v0.4 — search highlight clears on edit; status timeout
// ----------------------------------------------------------------------

#[test]
fn search_highlight_clears_on_edit_and_returns_on_new_search() {
    let mut e = ed_with("cat dog cat");
    e.handle_input(Input::Char('/'));
    for c in "cat".chars() {
        e.handle_input(Input::Char(c));
    }
    e.handle_input(Input::Enter);
    assert_eq!(e.search_ranges(0).len(), 2);
    e.handle_input(Input::Char('x')); // any edit (deletes first 'c' -> "at dog cat")
    assert!(
        e.search_ranges(0).is_empty(),
        "highlight must clear on edit"
    );
    // n still works after the edit and re-arms highlighting
    e.handle_input(Input::Char('n'));
    assert_eq!(e.search_ranges(0).len(), 1, "n re-arms highlighting");
}

#[test]
fn transient_status_fades_help_persists() {
    use std::time::Duration;
    let mut e = ed(); // WELCOME is permanent
    assert!(e.status.contains("HELP"));
    assert!(!e.tick_status());
    assert!(e.status.contains("HELP"));

    e.status_timeout = Duration::ZERO;
    e.handle_input(Input::Char('i')); // transient "-- INSERT --"
    assert!(e.status.contains("INSERT"));
    assert!(e.tick_status(), "transient status must clear once expired");
    assert!(e.status.is_empty());
    // second tick: nothing to clear
    assert!(!e.tick_status());
}

#[test]
fn undo_after_dd_on_last_line_restores_sane_cursor() {
    // Regression guard: dd on the last line, then undo — cursor must land on
    // the line that took the deleted one's place (vim behaviour), never panic.
    let mut e = ed_with("a\nb\nc");
    feed(
        &mut e,
        &[Input::Char('G'), Input::Char('d'), Input::Char('d')],
    );
    assert_eq!(text(&e), "a\nb");
    assert_eq!(e.cy, 1);
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "a\nb\nc");
    assert!(e.cy <= 2);
}

// ----------------------------------------------------------------------
// v0.5 — count-prefixed motions and operators
// ----------------------------------------------------------------------

#[test]
fn count_multiplies_motions() {
    let content: String = (0..20).map(|i| format!("line{i}\n")).collect();
    let mut e = ed_with(&content);
    feed(&mut e, &[Input::Char('5'), Input::Char('j')]);
    assert_eq!(e.cy, 5);
    feed(&mut e, &[Input::Char('3'), Input::Char('k')]);
    assert_eq!(e.cy, 2);
    // counts accumulate: 12j = 2+... from line 2 -> 12
    feed(
        &mut e,
        &[Input::Char('1'), Input::Char('2'), Input::Char('j')],
    );
    assert_eq!(e.cy, 14);
}

#[test]
fn count_zero_is_line_start_not_count() {
    let mut e = ed_with("abc");
    feed(
        &mut e,
        &[Input::Char('l'), Input::Char('l'), Input::Char('0')],
    );
    assert_eq!(e.cx, 0);
}

#[test]
#[allow(non_snake_case)]
fn count_G_and_gg_goto_line() {
    let content: String = (0..20).map(|i| format!("line{i}\n")).collect();
    let mut e = ed_with(&content);
    feed(&mut e, &[Input::Char('7'), Input::Char('G')]);
    assert_eq!(e.cy, 6);
    feed(
        &mut e,
        &[
            Input::Char('1'),
            Input::Char('5'),
            Input::Char('g'),
            Input::Char('g'),
        ],
    );
    assert_eq!(e.cy, 14);
    // plain gg still goes to line 1
    feed(&mut e, &[Input::Char('g'), Input::Char('g')]);
    assert_eq!(e.cy, 0);
}

#[test]
fn count_dd_deletes_n_lines_as_one_undo() {
    let mut e = ed_with("a\nb\nc\nd\ne");
    feed(
        &mut e,
        &[Input::Char('2'), Input::Char('d'), Input::Char('d')],
    );
    assert_eq!(text(&e), "c\nd\ne"); // lines "a","b" removed
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "a\nb\nc\nd\ne");
}

#[test]
fn count_yy_yanks_n_lines_and_pastes() {
    let mut e = ed_with("a\nb\nc");
    feed(
        &mut e,
        &[
            Input::Char('2'),
            Input::Char('y'),
            Input::Char('y'),
            Input::Char('p'),
        ],
    );
    assert_eq!(text(&e), "a\na\nb\nb\nc"); // pasted after line "a"
}

#[test]
fn count_x_deletes_n_chars() {
    let mut e = ed_with("abcdef");
    feed(&mut e, &[Input::Char('3'), Input::Char('x')]);
    assert_eq!(text(&e), "def");
    // more than available: stops at line end
    feed(&mut e, &[Input::Char('9'), Input::Char('x')]);
    assert_eq!(text(&e), "");
}

#[test]
fn count_p_pastes_n_times() {
    let mut e = ed_with("ab");
    feed(
        &mut e,
        &[
            Input::Char('y'),
            Input::Char('y'),
            Input::Char('2'),
            Input::Char('p'),
        ],
    );
    assert_eq!(text(&e), "ab\nab\nab");
}

#[test]
fn count_is_reset_by_uncounatable_keys() {
    let mut e = ed_with("a\nb\nc\nd");
    feed(&mut e, &[Input::Char('3'), Input::Char('u')]); // 3 dropped
    feed(&mut e, &[Input::Char('j')]);
    assert_eq!(e.cy, 1);
}

// ----------------------------------------------------------------------
// v0.5 — visual mode
// ----------------------------------------------------------------------

#[test]
fn visual_select_and_delete_single_line() {
    let mut e = ed_with("hello world");
    // v, 4 l (over "hell" + 'o'? v then 4l selects h..o cols 0..4), d
    feed(
        &mut e,
        &[
            Input::Char('v'),
            Input::Char('4'),
            Input::Char('l'),
            Input::Char('d'),
        ],
    );
    assert_eq!(text(&e), " world");
    assert_eq!(e.mode, Mode::Normal);
    // register holds the selection
    match &e.register {
        Some(as_vim::editor::Register::Chars(c)) => {
            let s: String = c.iter().collect();
            assert_eq!(s, "hello");
        }
        other => panic!("unexpected register {other:?}"),
    }
    // one undo step
    e.handle_input(Input::Char('u'));
    assert_eq!(text(&e), "hello world");
}

#[test]
fn visual_yank_and_paste_multiline() {
    let mut e = ed_with("alpha\nbeta\ngamma");
    feed(
        &mut e,
        &[
            Input::Char('v'),
            Input::Char('j'),
            Input::Char('l'),
            Input::Char('y'),
        ],
    );
    assert_eq!(e.mode, Mode::Normal);
    // cursor on "beta" col 1; selection was "alpha\nbe"; p after cursor char
    e.handle_input(Input::Char('p'));
    assert_eq!(text(&e), "alpha\nbealpha\nbeta\ngamma");
}

#[test]
fn visual_delete_across_lines() {
    let mut e = ed_with("keep1 cut1\ncut2 keep2");
    // select from 'c' of cut1 (line0 col6) to '2' of cut2 (line1 col3)
    e.handle_input(Input::Char('$')); // end of line 0
    e.handle_input(Input::Char('v'));
    e.handle_input(Input::Char('j'));
    e.handle_input(Input::Char('0'));
    e.handle_input(Input::Char('l'));
    e.handle_input(Input::Char('l'));
    e.handle_input(Input::Char('l')); // col 3
    e.handle_input(Input::Char('d'));
    assert_eq!(text(&e), "keep1 cut keep2");
}

#[test]
fn visual_esc_cancels_and_v_toggles() {
    let mut e = ed_with("abc");
    e.handle_input(Input::Char('v'));
    assert_eq!(e.mode, Mode::Visual);
    e.handle_input(Input::Char('v'));
    assert_eq!(e.mode, Mode::Normal);
    e.handle_input(Input::Char('v'));
    e.handle_input(Input::Esc);
    assert_eq!(e.mode, Mode::Normal);
    assert_eq!(text(&e), "abc");
    assert!(!e.dirty());
}

#[test]
fn visual_selection_range_normalization() {
    // selecting backwards: anchor right, cursor left
    let mut e = ed_with("abcdef");
    feed(
        &mut e,
        &[
            Input::Char('$'),
            Input::Char('v'),
            Input::Char('h'),
            Input::Char('h'),
        ],
    );
    let ((y0, x0), (y1, x1)) = e.visual_range();
    assert_eq!((y0, x0), (0, 3)); // cursor on col 3 after two h from $
    assert_eq!((y1, x1), (0, 5));
}

#[test]
fn visual_o_swaps_ends() {
    let mut e = ed_with("abcdef");
    feed(
        &mut e,
        &[Input::Char('v'), Input::Char('l'), Input::Char('o')],
    );
    // anchor now at old cursor (1); cursor back at anchor (0)
    assert_eq!(e.visual_anchor, (1, 0));
    assert_eq!(e.cx, 0);
}

#[test]
fn visual_delete_selection_shrinks_to_cursor() {
    // delete selection that ends mid-word
    let mut e = ed_with("one two three");
    feed(
        &mut e,
        &[
            Input::Char('v'),
            Input::Char('5'),
            Input::Char('l'),
            Input::Char('x'),
        ],
    );
    // cols 0..5 = "one tw" deleted -> "o three"
    assert_eq!(text(&e), "o three");
}

#[test]
fn paste_char_register_with_newlines_from_visual_yank() {
    let mut e = ed_with("aa\nbb");
    feed(
        &mut e,
        &[
            Input::Char('v'),
            Input::Char('j'),
            Input::Char('$'),
            Input::Char('y'),
        ],
    );
    // selection "aa\nbb"; move to line 3? buffer has 2 lines; go to end
    feed(
        &mut e,
        &[Input::Char('G'), Input::Char('$'), Input::Char('p')],
    );
    // paste at end of "bb" -> "bb" + newline? selection ends at 'b' (col 1)
    // pasted text "aa\nbb" after the final 'b': "bbaa" / "bb"
    assert_eq!(text(&e), "aa\nbbaa\nbb"); // vim p: after cursor char
}
