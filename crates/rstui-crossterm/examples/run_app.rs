//! End to end: an unmodified rstui app on a real terminal.
//!
//! This is the capstone the preceding slices built toward. It composes every
//! seam at once:
//!
//! - a panic-safe [`TerminalGuard`] wrapping a [`CrosstermBackend`] over
//!   stdout, forming one `Terminal -> TerminalGuard -> CrosstermBackend ->
//!   Stdout` ownership chain that restores the terminal on exit *and* on panic;
//! - a [`CrosstermEventSource`] reading real keystrokes and translating them
//!   into rstui's event vocabulary;
//! - `rstui_runtime::run` — the *same* function the headless harness tests
//!   drive over a `TestBackend` + `TestEventSource`. The `App` below is
//!   byte-for-byte the kind of reducer those tests exercise; nothing about it
//!   is terminal-specific.
//!
//! Run it in a real terminal:
//!
//! ```text
//! cargo run -p rstui-crossterm --example run_app
//! ```
//!
//! `+`/`=` increments, `-` decrements, `q` or `Esc` quits. It needs a TTY, so
//! CI builds it (proving the whole stack type-checks and composes) but does not
//! execute it.

use std::error::Error;
use std::io::{self, Stdout};

use rstui_core::{Color, KeyCode, Style};
use rstui_crossterm::{CrosstermBackend, CrosstermEventSource, TerminalGuard};
use rstui_runtime::{App, Cmd, Event, Frame, run};

#[derive(Default)]
struct Counter {
    value: i64,
}

enum Msg {
    Inc,
    Dec,
    Quit,
}

impl App for Counter {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event.as_key_press()?.code {
            KeyCode::Char('+') | KeyCode::Char('=') => Some(Msg::Inc),
            KeyCode::Char('-') => Some(Msg::Dec),
            KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Inc => {
                self.value += 1;
                Cmd::none()
            }
            Msg::Dec => {
                self.value -= 1;
                Cmd::none()
            }
            Msg::Quit => Cmd::quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let pos = frame.area().position();
        let line = format!(
            " rstui live — value = {}   (+/- change · q quit) ",
            self.value
        );
        frame
            .buffer_mut()
            .set_str(pos, &line, Style::new().fg(Color::Green));
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // One panic-safe ownership chain. The guard enters raw mode + the alternate
    // screen on construction and restores them when `run` drops the terminal —
    // including while unwinding from a panic.
    let backend: CrosstermBackend<Stdout> = CrosstermBackend::new(io::stdout());
    let guard = TerminalGuard::new(backend)?;

    // The identical `run` the harness tests call — here over the live terminal.
    // `?` bubbles a `RunError` (it is `std::error::Error`); the guard's `Drop`
    // has already restored the terminal by the time this returns either way.
    run(Counter::default(), guard, &mut CrosstermEventSource::new())?;
    Ok(())
}
