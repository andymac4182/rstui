//! End to end: an unmodified rstui app on a real terminal, in one call.
//!
//! This is the capstone the preceding slices built toward. Every seam still
//! composes at once — a panic-safe [`TerminalGuard`] over a
//! [`CrosstermBackend`], a [`CrosstermEventSource`], and the *same*
//! `rstui_runtime::run` the headless harness tests drive — but the app no
//! longer hand-wires them: [`run_app`] owns that composition and installs the
//! panic-restore hook, so a crash leaves the terminal clean *and* the panic
//! message readable on the user's normal screen.
//!
//! Run it in a real terminal:
//!
//! ```text
//! cargo run -p rstui-crossterm --example run_app
//! ```
//!
//! `+`/`=` increments, `-` decrements, `q` or `Esc` quits. `!` panics on
//! purpose — quit that way and the message is still readable, proving the
//! panic policy. It needs a TTY, so CI builds it (proving the whole stack
//! type-checks and composes) but does not execute it.

use std::error::Error;

use rstui_core::{Color, KeyCode, Style};
use rstui_crossterm::run_app;
use rstui_runtime::{App, Cmd, Event, Frame};

#[derive(Default)]
struct Counter {
    value: i64,
}

enum Msg {
    Inc,
    Dec,
    /// Deliberately panic, to demonstrate that the panic-restore hook leaves
    /// the terminal usable *and* the panic message visible.
    Boom,
    Quit,
}

impl App for Counter {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event.as_key_press()?.code {
            KeyCode::Char('+') | KeyCode::Char('=') => Some(Msg::Inc),
            KeyCode::Char('-') => Some(Msg::Dec),
            KeyCode::Char('!') => Some(Msg::Boom),
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
            Msg::Boom => panic!("intentional panic at value = {}", self.value),
            Msg::Quit => Cmd::quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let pos = frame.area().position();
        let line = format!(
            " rstui live — value = {}   (+/- change · ! panic · q quit) ",
            self.value
        );
        frame
            .buffer_mut()
            .set_str(pos, &line, Style::new().fg(Color::Green));
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // The whole stack — alternate screen, raw mode, mouse/paste/focus capture,
    // panic-safe restore, the live event loop — in one call. `?` bubbles a
    // `CrosstermRunError`; the terminal is already restored by the time it
    // returns, on success, error, or panic.
    run_app(Counter::default())?;
    Ok(())
}
