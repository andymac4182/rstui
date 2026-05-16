//! A complete counter app driven by the headless [`Harness`], so it runs
//! deterministically with no terminal and doubles as a smoke test of the
//! `App`/`Cmd`/`Harness` loop:
//!
//! ```text
//! cargo run -p rstui-runtime --example counter
//! ```
//!
//! It shows the whole contract end to end: `on_event` mapping keys to
//! intents, `update` mutating state, a `Cmd` feeding a follow-up message back
//! in, `view` rendering, and `Cmd::quit` stopping the loop.

use rstui_core::{KeyCode, KeyEvent, Style};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};

#[derive(Default)]
struct Counter {
    value: i64,
}

enum Msg {
    Increment,
    Decrement,
    /// Reset to zero, then re-increment once via a command (demonstrates the
    /// effect → message feedback loop without any async).
    ResetThenBump,
    Quit,
}

impl App for Counter {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Char('+') => Some(Msg::Increment),
            KeyCode::Char('-') => Some(Msg::Decrement),
            KeyCode::Char('r') => Some(Msg::ResetThenBump),
            KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Increment => {
                self.value += 1;
                Cmd::none()
            }
            Msg::Decrement => {
                self.value -= 1;
                Cmd::none()
            }
            Msg::ResetThenBump => {
                self.value = 0;
                Cmd::message(Msg::Increment)
            }
            Msg::Quit => Cmd::quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let pos = frame.area().position();
        frame.buffer_mut().set_str(
            pos,
            &format!("count: {:>3} (+/- r q)", self.value),
            Style::new(),
        );
    }
}

fn main() {
    let mut harness = Harness::new(Counter::default(), 22, 1);
    println!("start    -> {}", harness.snapshot().trim_end());

    // A scripted session: each key flows through on_event -> update -> view.
    let script = [
        ('+', "increment"),
        ('+', "increment"),
        ('+', "increment"),
        ('-', "decrement"),
        ('r', "reset, then a Cmd re-increments"),
        ('q', "quit"),
        ('+', "ignored: app already quit"),
    ];

    for (key, note) in script {
        harness.handle(Event::from(KeyEvent::char(key)));
        println!(
            "key '{key}' -> {} | running={} ({note})",
            harness.snapshot().trim_end(),
            harness.is_running(),
        );
    }

    assert_eq!(harness.app().value, 1, "reset-then-bump lands on 1");
    assert!(!harness.is_running(), "q must have quit the app");
}
