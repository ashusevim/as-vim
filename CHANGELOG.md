# Changelog

## 0.5.0 — 2026-10-05

### Added
- **Count prefixes** — `3j`, `5l`, `2dd`, `3yy`, `4x`, `3p`, `12G`, `5gg`,
  `2PageUp`; counts accumulate (`12j`) and `0` stays the line-start
  motion. Operators run as one undo step.
- **Visual mode (charwise)** — `v` + motions to select, `d`/`x` delete,
  `y` yank (OSC 52), `o` swaps ends, `Esc`/`v` exits. Selections span
  lines; multi-line yanks paste with vim's inline-join semantics; every
  operation is one undo transaction. Selection renders as reverse video.

## 0.4.0 — 2026-10-05

### Added
- **Substitute**: `:s/old/new/` (first match on the current line),
  `:s/old/new/g` (all matches on the line), and `:%s/old/new/g` (whole
  buffer) — smart-case like `/`, `\/` and `\\` escapes, empty pattern
  reuses the last search, empty replacement deletes, and the whole
  command is one undo step. Reports `N substitution(s) on M lines`.

### Changed
- `/`-search highlighting now clears on the next buffer edit (vim
  hlsearch behaviour); `n`/`N` re-arm it.
- Status messages fade after 3 seconds (help text stays); the event
  loop polls instead of blocking so messages fade without keypresses.

## 0.3.1 — 2026-10-04

### Changed
- README repositioned around the two target use cases (drop-in editor for
  containers/rescue boxes; safe `$EDITOR` for git/crontab/kubectl) with a
  comparison table and a working `releases/latest/download` quick start.
- Release workflow also uploads versionless archives
  (`as-vim-<target>.tar.gz`) so the latest-release links stay stable.

No code changes — this release exists to sync the crates.io page with
the GitHub README.

## 0.3.0 — 2026-10-04

### Added
- **Syntax highlighting** for Rust, C/C++, JavaScript/TypeScript, Python,
  Go, Java, Shell, JSON, and TOML — detected by file extension (and by
  `:w <file>` when saving to a new name). Keywords, types, strings,
  numbers, comments, shell `$vars`, and C preprocessor directives are
  colored; block comments track state across lines. Zero new
  dependencies — hand-rolled per-language scanners.

## 0.2.0 — 2026-10-04

### Added
- **Undo/redo** (`u`, `Ctrl+r`, `:undo`, `:redo`) with vim-style change
  grouping: consecutive typing coalesces into one step, `dd` and
  `o`+typing each undo as a unit, cursor position is restored.
- **Search** (`/`) with incremental prompt, smart-case matching
  (case-insensitive unless the pattern has uppercase), `n`/`N` repeat
  with buffer wrap-around, and in-buffer match highlighting.
- **Yank/paste** (`yy`, `p`, `P`) with a vim-style unnamed register;
  `x` and `dd` also fill the register. Explicit yanks are copied to the
  system clipboard via OSC 52, which works over SSH.
- Dirty indicator in the status bar is now exact: it clears when undoing
  back to the saved state, not just on save.

### Changed
- Welcome/status hints updated for the new commands.

## 0.1.0 — 2026-10-04

- Initial release: Rust rewrite of as-nano.
- NORMAL/INSERT/COMMAND modes, `hjkl` + arrows, `i a A o O x dd`,
  `0 $ gg G`, `:w :q :q! :wq :x`, scrolling, resize handling,
  dirty-flag `:q` guard, multi-byte-safe editing.
- Single static binaries for Linux (musl x86_64/aarch64), macOS
  (x86_64/aarch64), Windows via GitHub Releases; `cargo install as-vim`.
