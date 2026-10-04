# as-vim

A minimal, fast, vim-inspired terminal text editor — one tiny, hackable Rust codebase, one static binary.

> The original was written in TypeScript (`legacy/`); as-vim is the Rust rewrite with a
> single-binary distribution, scrolling, and a dirty-state guard.

## Features

- **Modal editing** — NORMAL / INSERT / COMMAND, vim-style
- **Undo/redo** — `u` / `Ctrl+r`, vim-style change grouping (`dd`, `o`+typing, word-runs each undo as one step)
- **Search** — `/` with incremental prompt, `n`/`N` to repeat, wrap-around, match highlighting, smart-case
- **Yank/paste + system clipboard** — `yy`, `p`/`P`, `x`/`dd` fill the unnamed register; yanks sync to the system clipboard via OSC 52 (works over SSH)
- **Motions** — `h j k l`, arrow keys, `0`, `$`, `gg`, `G`, `PageUp`/`PageDown`
- **Editing** — `i` `a` `A` `o` `O`, `x`, `dd`, full multi-byte (emoji, CJK) safety
- **Ex commands** — `:w`, `:w <file>`, `:q`, `:q!`, `:wq`, `:x`
- **Safe defaults** — `:q` refuses to discard unsaved changes; dirty indicator `[+]` in the status bar
- **Scrolling** — files bigger than the viewport scroll vertically *and* horizontally
- **Resize-aware** — reflows when you resize the terminal
- **Unicode correct** — wide characters and tabs render and cursor-track properly

## Install

### From crates.io

```bash
cargo install as-vim
```

### Prebuilt binaries

Grab a static binary for Linux (x86_64/aarch64), macOS (Intel/Apple Silicon), or Windows from the
[releases page](https://github.com/ashusevim/as-vim/releases):

```bash
# example: Linux x86_64
curl -LO https://github.com/ashusevim/as-vim/releases/latest/download/as-vim-x86_64-unknown-linux-musl.tar.gz
tar xzf as-vim-x86_64-unknown-linux-musl.tar.gz
sudo mv as-vim-*/as-vim /usr/local/bin/
```

### From source

```bash
git clone https://github.com/ashusevim/as-vim
cd as-vim
cargo install --path .
```

## Usage

```bash
as-vim notes.txt     # open (or create on save)
as-vim               # empty unnamed buffer, use :w <file> to save
```

## Keys

### NORMAL

| Key | Action |
|-----|--------|
| `h` `j` `k` `l` / arrows | move cursor |
| `0` / `$` | line start / line end |
| `gg` / `G` | buffer start / buffer end |
| `Enter` | next line, first column |
| `i` / `a` / `A` | insert before / after cursor / at line end |
| `o` / `O` | open line below / above |
| `x`, `Delete` | delete char under cursor |
| `dd` | delete line (into unnamed register) |
| `yy` | yank line (also → system clipboard via OSC 52) |
| `p` / `P` | paste register after / before cursor |
| `u` / `Ctrl+r` | undo / redo |
| `/` | search (then `Enter`, `n`, `N`) |
| `:` | enter COMMAND mode |
| `Ctrl+C` | hint (nothing is force-killed) |

### INSERT

| Key | Action |
|-----|--------|
| printable chars | insert at cursor |
| `Enter` | split line at cursor |
| `Backspace` | delete before cursor / join previous line |
| `Delete` | delete at cursor / join next line |
| `Esc` | back to NORMAL |

### COMMAND

| Command | Action |
|---------|--------|
| `:w [file]` | save (optionally to a new file) |
| `:q` | quit (warns if unsaved changes) |
| `:q!` | quit discarding changes |
| `:wq` / `:x` | save and quit |
| `:undo` / `:redo` | undo / redo |
| `Esc` | cancel command |

## Design principles

- State is minimal and explicit; rendering is a pure function of that state
- Editor logic is terminal-agnostic and unit-tested (no pty needed for the test suite)
- Small surface area — read `src/editor.rs` and you have read the editor

## Development

```bash
cargo test        # 38 tests, no terminal required
cargo clippy      # lint
cargo build --release
```

## Roadmap

- [x] Undo/redo (`u`, `Ctrl+r`)
- [x] Search (`/`, `n`, `N`) with highlighting
- [x] System clipboard over SSH (OSC 52)
- [ ] `:s` substitute / search with replacement
- [ ] Count-prefixed motions (`5j`, `3dd`)
- [ ] Syntax highlighting
- [ ] Visual mode (`v` + `d`/`y`)

## License

MIT — see [LICENSE](LICENSE).
