//! as-vim — a minimal, vim-inspired terminal text editor.
//!
//! Binary entry point: CLI parsing, terminal setup/teardown, event loop.
//! All editor logic lives in [`as_vim::editor`].

use std::io::{self, Write};
use std::path::Path;

use clap::Parser;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, size, EnterAlternateScreen, LeaveAlternateScreen,
};

use as_vim::editor::{Editor, Effect};
use as_vim::input::{poll_event, read_terminal_event, TerminalEvent};
use as_vim::ui;

use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "as-vim",
    version,
    about = "A minimal, vim-inspired terminal text editor",
    after_help = "Keys: i insert | Esc normal | :w save | :q quit | :q! force quit | :wq save & quit"
)]
struct Cli {
    /// File to open (created on save if missing). Omit for an empty buffer.
    file: Option<String>,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    install_panic_hook();

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    let result = run(&cli, &mut stdout);

    restore_terminal()?;
    result
}

fn run(cli: &Cli, stdout: &mut io::Stdout) -> io::Result<()> {
    let (mut cols, mut rows) = size()?;

    let mut ed = match &cli.file {
        Some(path) => Editor::open(Path::new(path), rows as usize, cols as usize)?,
        None => Editor::new(None, rows as usize, cols as usize),
    };

    // Wait for a usable terminal size.
    while rows < 3 || cols < 10 {
        if let TerminalEvent::Resize = read_terminal_event()? {
            (cols, rows) = size()?;
            ed.screen_rows = rows as usize;
            ed.screen_cols = cols as usize;
        }
    }

    ui::refresh_screen(stdout, &mut ed)?;

    loop {
        match poll_event(Duration::from_millis(200))? {
            Some(TerminalEvent::Resize) => {
                let (c, r) = size()?;
                ed.screen_rows = r as usize;
                ed.screen_cols = c as usize;
            }
            Some(TerminalEvent::Input(input)) => {
                if ed.handle_input(input) == Effect::Quit {
                    return Ok(());
                }
                // Yank → system clipboard via OSC 52 (works over SSH).
                if let Some(text) = ed.take_clipboard() {
                    let b64 = as_vim::editor::base64_encode(text.as_bytes());
                    write!(stdout, "\x1b]52;c;{b64}\x07")?;
                }
            }
            // Idle tick: fade transient status messages.
            None => {
                if ed.tick_status() {
                    ui::refresh_screen(stdout, &mut ed)?;
                }
                continue;
            }
        }
        ui::refresh_screen(stdout, &mut ed)?;
    }
}

fn restore_terminal() -> io::Result<()> {
    let mut stdout = io::stdout();
    disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)
}

/// Make sure a panic doesn't leave the user's terminal in raw mode.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));
}
