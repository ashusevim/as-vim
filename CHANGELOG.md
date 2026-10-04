# Changelog

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
