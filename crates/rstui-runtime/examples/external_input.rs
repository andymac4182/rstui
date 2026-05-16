//! The live [`run`] loop driven by a *real other thread* feeding input over a
//! channel, with no terminal — proving the [`EventSource`] boundary is not
//! crossterm-only:
//!
//! ```text
//! cargo run -p rstui-runtime --example external_input
//! ```
//!
//! [`counter`](../counter/index.html) and [`spinner`](../spinner/index.html)
//! exercise the loop through the headless [`Harness`]; this one exercises the
//! *real* [`run`] entry point with a [`ChannelEventSource`] in place of
//! crossterm. A background `std::thread` plays a scripted key sequence into the
//! channel; `run` blocks on it exactly as it would on a TTY, and when the
//! producer thread finishes it drops its [`Sender`](std::sync::mpsc::Sender),
//! closing the channel so `poll_event(None)` returns end-of-input and the loop
//! stops on its own — the same `App`/`Cmd` code, a different source. The result
//! stays deterministic because the producer sends a *fixed* script and we
//! `join` it, so there is no wall clock or race despite the second thread.

use std::thread;

use rstui_core::{ChannelEventSource, KeyCode, KeyEvent, Style, TestBackend};
use rstui_runtime::{App, Cmd, Event, Frame, run};

#[derive(Default)]
struct Counter {
    value: i64,
}

enum Msg {
    Increment,
    Decrement,
    /// Reset to zero, then re-increment once via a command (the same
    /// effect → message feedback the `counter` example shows, kept so this
    /// proves the *whole* contract still holds under a threaded source).
    ResetThenBump,
}

impl App for Counter {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event.as_key_press()?.code {
            KeyCode::Char('+') => Some(Msg::Increment),
            KeyCode::Char('-') => Some(Msg::Decrement),
            KeyCode::Char('r') => Some(Msg::ResetThenBump),
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
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let pos = frame.area().position();
        frame
            .buffer_mut()
            .set_str(pos, &format!("count: {:>3}", self.value), Style::new());
    }
}

/// The scripted keystrokes the producer thread sends, in order. No quit key:
/// the loop is meant to stop on *end-of-input* when the channel closes, which
/// is the half of the [`EventSource`] contract a threaded source adds over the
/// scripted one.
const SCRIPT: [char; 6] = ['+', '+', '+', '-', 'r', '+'];

/// Runs the counter under the real [`run`] loop with input arriving from
/// another thread, and returns the final app. Shared by `main` and the test so
/// they assert the identical deterministic outcome.
fn run_with_threaded_input() -> Counter {
    let (mut input, tx) = ChannelEventSource::new();

    // A producer thread plays the fixed script, then *returns* — dropping the
    // only sender, which closes the channel. `run`'s unbounded `poll_event`
    // then yields `Ok(None)` (end-of-input) and the loop exits cleanly.
    let producer = thread::spawn(move || {
        for key in SCRIPT {
            tx.send(Event::from(KeyEvent::char(key)))
                .expect("run loop holds the receiver for the whole script");
        }
    });

    // 22x1 is plenty for "count: NNN"; the backend is in-memory, no TTY.
    let app = run(Counter::default(), TestBackend::new(22, 1), &mut input)
        .expect("TestBackend + ChannelEventSource are both Infallible");

    producer.join().expect("producer thread must not panic");
    app
}

fn main() {
    let app = run_with_threaded_input();
    println!("script {SCRIPT:?} over a channel from another thread");
    println!(
        "final  -> count: {} (loop stopped on end-of-input)",
        app.value
    );

    // +,+,+ -> 3; - -> 2; r resets to 0 then a Cmd re-increments -> 1; + -> 2.
    assert_eq!(app.value, 2, "the scripted threaded session lands on 2");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threaded_input_drives_the_real_run_loop_deterministically() {
        let app = run_with_threaded_input();
        assert_eq!(app.value, 2);
    }

    /// The loop must stop because the channel *closed* (every sender dropped),
    /// not because of a quit key — there is none in `SCRIPT`. Re-running is
    /// deterministic, which is the property the `join` + fixed script buy
    /// despite the extra thread.
    #[test]
    fn run_exits_on_channel_close_and_is_repeatable() {
        assert_eq!(run_with_threaded_input().value, 2);
        assert_eq!(run_with_threaded_input().value, 2);
    }
}
