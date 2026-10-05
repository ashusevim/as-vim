//! Maps crossterm events onto the backend-independent [`Input`] enum.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::editor::Input;

/// Something the main loop needs to react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalEvent {
    Input(Input),
    Resize,
}

/// Wait up to `timeout` for a meaningful terminal event.
/// Returns `Ok(None)` when nothing arrived (caller may tick timers).
pub fn poll_event(timeout: Duration) -> io::Result<Option<TerminalEvent>> {
    if !event::poll(timeout)? {
        return Ok(None);
    }
    loop {
        match event::read()? {
            Event::Resize(..) => return Ok(Some(TerminalEvent::Resize)),
            Event::Key(key)
                // Windows sends Release/Repeat events; act on presses and repeats.
                if key.kind != KeyEventKind::Release =>
            {
                if let Some(input) = map_key(key) {
                    return Ok(Some(TerminalEvent::Input(input)));
                }
            }
            // FocusGained/Lost, Mouse, Paste — ignore.
            _ => {}
        }
        // We consumed an event that mapped to nothing; drain what's already
        // queued before reporting an idle tick.
        if !event::poll(Duration::ZERO)? {
            return Ok(None);
        }
    }
}

/// Block until a meaningful terminal event arrives.
pub fn read_terminal_event() -> io::Result<TerminalEvent> {
    loop {
        if let Some(ev) = poll_event(Duration::from_secs(1))? {
            return Ok(ev);
        }
    }
}

fn map_key(key: KeyEvent) -> Option<Input> {
    use Input::*;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Char('c') if ctrl => Some(Ctrl('c')),
        KeyCode::Char(c) if ctrl => Some(Ctrl(c)),
        KeyCode::Char(c) if !alt => Some(Char(c)),
        KeyCode::Enter => Some(Enter),
        KeyCode::Backspace => Some(Backspace),
        KeyCode::Esc => Some(Esc),
        KeyCode::Tab => Some(Tab),
        KeyCode::Delete => Some(Delete),
        KeyCode::Up => Some(ArrowUp),
        KeyCode::Down => Some(ArrowDown),
        KeyCode::Left => Some(ArrowLeft),
        KeyCode::Right => Some(ArrowRight),
        KeyCode::Home => Some(Home),
        KeyCode::End => Some(End),
        KeyCode::PageUp => Some(PageUp),
        KeyCode::PageDown => Some(PageDown),
        _ => None,
    }
}
