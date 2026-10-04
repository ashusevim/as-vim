# as-nano

Minimal modal terminal text editor (Vim‑inspired) written in TypeScript / Node.js. Opens a file into a line buffer, lets you navigate, edit, and write back to disk using a tiny, hackable codebase.

## Install

```bash
git clone <repo-url>
cd as-nano
npm install
npm run build   # if a build step is added later; currently source can run directly
npm link        # optional: exposes the CLI globally as `as-nano`
```

## Run

```bash
# Open (or create) a file
as-nano notes.txt

# Or with node directly (dev mode)
node src/index.ts notes.txt
```

If no filename is provided it starts with an empty unnamed buffer.

## Modes

| Mode | Enter | Exit | Purpose |
|------|-------|------|---------|
| NORMAL | (startup) | `i`, `:` (to switch) | Navigation / high-level commands |
| INSERT | `i` from NORMAL | `Esc` | Text entry / editing |
| COMMAND | `:` from NORMAL | `Enter` (exec) / `Esc` (cancel) | Ex commands like save & quit |

## Keys (current implementation)

NORMAL:
* `i` – enter INSERT
* `:` – enter COMMAND
* Arrow keys (or planned `h j k l`) – move cursor
* `Esc` in COMMAND returns to NORMAL (also cancels command)

INSERT:
* Type printable characters to insert at the cursor
* `Enter` – split line at cursor (text to the right becomes new line)
* `Backspace` – delete char before cursor; at column 0 join with previous line
* `Esc` – back to NORMAL; cursor adjusted if it sat past end of line

COMMAND (starts with a `:` prompt on status bar):
* `:w` – write file
* `:q` – quit (fails if unsaved changes logic added later)
* `:wq` – write and quit
* `Esc` – cancel command input

## UI Layout

1. Text area (all rows except last one) – file buffer lines or `~` markers after EOF
2. Status bar (last row) – inverted colors; shows either:
	* COMMAND mode: `:` followed by current command input buffer (`commandString`)
	* Other modes: `-- MODE -- | line:col | path`
3. Optional status/help line (second-to-last row when not in COMMAND) – displays `Editor.status`

## Core Data Structures

`Editor` (singleton object):
* `filePath` – current file path or empty
* `lines: string[]` – each file line (no trailing newline characters stored)
* `cursorX`, `cursorY` – zero-based column & row within `lines`
* `screenRows`, `screenCols` – snapshot of terminal size from `process.stdout`
* `mode` – one of `NORMAL | INSERT | COMMAND`
* `commandString` – buffer while typing after `:`
* `status` – message/info line

`Terminal` utilities wrap raw ANSI escape sequences:
* `clearScreen()` – ESC[2J (clear) + ESC[H (home)
* `moveCursor(r,c)` – ESC[<r+1>;<c+1>H (term is 1-based)
* `invertColors()` – ESC[7m reverse video (used for status bar)
* `resetColors()` – ESC[m reset attributes

## Rendering Cycle (`render()`)

1. Clear screen
2. Draw each visible row: real line text or `~` placeholder until `screenRows - 1`
3. Draw status bar (inverted) padded to full width with `padEnd` to overwrite leftovers
4. Draw help/status line (only outside COMMAND mode)
5. Position terminal cursor either inside text area or inside the command prompt

`padEnd(screenCols)` ensures shorter subsequent status lines overwrite previous longer ones.

## Input Handling

Raw mode (`process.stdin.setRawMode(true)`) lets the program receive keystrokes immediately (including escape sequences for arrows). A central keypress dispatcher routes input based on `Editor.mode` to specialized handlers (e.g., `handleInsertModeKeypress`).

### Insert Mode Line Split (Enter)
```
currentLine = lines[cursorY]
rightPart = currentLine.substring(cursorX)
lines[cursorY] = currentLine.substring(0, cursorX)
lines.splice(cursorY + 1, 0, rightPart)
cursorY++; cursorX = 0
```

### Insert Mode Backspace
* If `cursorX > 0`: remove char before cursor (string slice + concat)
* Else if not first line: merge current line into previous; adjust `cursorY` and `cursorX`

### Esc From Insert
Adjust `cursorX` if it ended up beyond new line end (mimics Vim moving from insert to normal at last character).

## Saving

Writing (`:w`) serializes `lines.join("\n")` to `filePath`. Basic error handling updates `status` with success/failure.

## Design Principles

* Keep state minimal & explicit
* Avoid external dependencies for terminal control (raw ANSI)
* Pure-ish rendering: UI derived solely from `Editor` state
* Small surface area to encourage experimentation

## Limitations

* No undo/redo
* No search (`/`), replace, or navigation shortcuts beyond arrows
* No scrolling (assumes file fits in viewport) – implement viewport offset (`rowOffset`) next
* No detection of terminal resize events
* No dirty flag / unsaved changes warning
* Backspace merge bug risk: ensure line concatenation uses previous + current (not duplicate previous)
* Add input for `h j k l`, `0`, `$`, `dd`, etc.

## Minimal Specification (Original Goal)

- Start the program
- Receive input to type characters
- Move cursor to edit document buffer
- Save document buffer to disk
- Load a file from disk

## License

others