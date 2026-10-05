# as-vim

**The smallest vim you can `scp` anywhere — and the `$EDITOR` that can't lose your commit message.**

A modal terminal text editor in a single ~920 KB static binary. No config file, no plugin
system, no runtime: ~2,500 lines of dependency-light Rust you can read in one sitting.

It exists for two moments:

1. **You're inside something with no editor.** A trimmed container, a rescue shell, a
   box where `nano`/`vim` aren't installed and the package manager is broken. Copy one
   static file in — `docker cp`, `scp`, or a single `curl` — and you get real vim keys
   instead of BusyBox `vi`'s stripped-down behavior.
2. **A tool opened an editor for you.** `git commit`, `crontab -e`, `kubectl edit`,
   `systemctl edit`, `sudoedit`. as-vim shows `:w`/`:q!` hints on screen, refuses to
   quit with unsaved changes (`:q!` to override), and marks the buffer `[+]` the
   moment it differs from disk — you can't lose your commit message by accident.

## Quick start

```bash
curl -LO https://github.com/ashusevim/as-vim/releases/latest/download/as-vim-x86_64-unknown-linux-musl.tar.gz
tar xzf as-vim-x86_64-unknown-linux-musl.tar.gz
./as-vim-*/as-vim notes.txt
```

Aarch64 Linux, macOS (Intel/Apple Silicon), and Windows binaries are on the
[releases page](https://github.com/ashusevim/as-vim/releases).

## Why as-vim and not ...

| | as-vim | nano | BusyBox `vi` | vim/Helix |
|---|---|---|---|---|
| vim keys | ✔ (subset) | ✘ | ✔ (hostile) | ✔ |
| static binary, zero deps | ~920 KB | needs ncurses | in BusyBox | multi-MB |
| won't quit on unsaved changes | ✔ always | ✘ | ✘ | ✔ with config |
| undo/redo, search, syntax highlighting | ✔ | ✔ | ✘ | ✔ |
| system clipboard over SSH (OSC 52) | ✔ built in | partial | ✘ | ✔ |
| config to maintain | none | `.nanorc` | none | `.vimrc`/toml |
| readable source, one sitting | ~2,500 lines | large | — | very large |

If you want LSP, splits, or a file tree, use [Helix](https://helix-editor.com) or
[Neovim](https://neovim.io) — as-vim deliberately stays out of that race.

## Features

- **Modal editing** — NORMAL / INSERT / COMMAND, vim-style
- **Undo/redo** — `u` / `Ctrl+r`, vim-style change grouping (`dd`, `o`+typing, word-runs each undo as one step)
- **Search** — `/` with incremental prompt, `n`/`N` to repeat, wrap-around, match highlighting, smart-case
- **Substitute** — `:s/old/new/`, `:s/../../g`, `:%s/old/new/g`; escapes, last-search reuse, one undo step per command
- **Yank/paste + system clipboard** — `yy`, `p`/`P`, `x`/`dd` fill the unnamed register; yanks sync to the system clipboard via OSC 52 (works over SSH)
- **Syntax highlighting** — Rust, C/C++, JavaScript/TypeScript, Python, Go, Java, Shell, JSON, TOML; zero dependencies, block comments track across lines
- **Motions** — `h j k l`, arrow keys, `0`, `$`, `gg`, `G`, `PageUp`/`PageDown`
- **Editing** — `i` `a` `A` `o` `O`, `x`, `dd`, full multi-byte (emoji, CJK) safety
- **Ex commands** — `:w`, `:w <file>`, `:q`, `:q!`, `:wq`, `:x`, `:undo`, `:redo`
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

# as the safe default editor
export EDITOR=as-vim   # git, crontab, kubectl, systemctl, sudoedit
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
| `:s/old/new/` | replace first match on the current line |
| `:s/old/new/g` | replace every match on the current line |
| `:%s/old/new/g` | replace across the whole buffer |
| `Esc` | cancel command |

## Design principles

- State is minimal and explicit; rendering is a pure function of that state
- Editor logic is terminal-agnostic and unit-tested (no pty needed for the test suite)
- Small surface area — read `src/editor.rs` and you have read the editor

## Development

```bash
cargo test        # 85 tests, no terminal required
cargo clippy      # lint
cargo build --release
```

## Roadmap

- [x] Undo/redo (`u`, `Ctrl+r`)
- [x] Search (`/`, `n`, `N`) with highlighting
- [x] System clipboard over SSH (OSC 52)
- [x] Syntax highlighting (9 file families)
- [x] `:s` substitute / search with replacement
- [ ] Count-prefixed motions (`5j`, `3dd`)
- [ ] Visual mode (`v` + `d`/`y`)

## License

MIT — see [LICENSE](LICENSE).
