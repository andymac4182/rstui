//! A "loading" screen driven by an off-loop [`Cmd::perform`] plus a
//! [`Cmd::tick`] retry — the scheduled-effect half of the runtime
//! (ADR 0007), shown deterministically:
//!
//! ```text
//! cargo run -p rstui-runtime --example background_load
//! cargo test -p rstui-runtime --examples
//! ```
//!
//! `init` kicks off a background fetch with [`Cmd::perform`]; while it is in
//! flight the screen shows a spinner-ish "loading"; on success it shows the
//! value, on failure it schedules a retry with [`Cmd::tick`]. The *same* app
//! runs three ways with one reducer:
//!
//! - under the headless [`Harness`] (and the default `run`) the executor is
//!   **inline**: `perform`/`tick` resolve immediately with zero virtual delay,
//!   so the whole flow is asserted with no clock — that is what the `main` and
//!   the `#[cfg(test)]` snapshots below do;
//! - under [`run_threaded`] the *identical* app runs each command on its own
//!   thread, so a genuinely slow fetch never freezes input/render and the
//!   retry `tick` actually waits — exercised by the threaded test at the
//!   bottom over a `TestBackend`.

use std::sync::atomic::{AtomicU32, Ordering};

use rstui_core::{KeyCode, KeyEvent, Style};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};

/// Flips to `false` once, so the scripted fetch "fails" exactly the first time
/// and the retry path (a real `Cmd::tick`) is exercised before it succeeds.
static FETCH_SUCCEEDS: AtomicU32 = AtomicU32::new(0);

fn fetch() -> Msg {
    // First call fails (returns the retry intent), every later call succeeds —
    // a deterministic stand-in for a flaky network load.
    if FETCH_SUCCEEDS.fetch_add(1, Ordering::Relaxed) == 0 {
        Msg::Failed
    } else {
        Msg::Loaded(42)
    }
}

#[derive(Default)]
enum Status {
    #[default]
    Loading,
    Retrying,
    Ready(u32),
}

#[derive(Default)]
struct Dashboard {
    status: Status,
    attempts: u32,
}

enum Msg {
    /// The background fetch finished with a value.
    Loaded(u32),
    /// The background fetch failed; schedule a retry.
    Failed,
    /// The retry timer elapsed: fetch again.
    Retry,
    Quit,
}

impl App for Dashboard {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        // Off-loop under `run_threaded`; immediate under Harness/`run`.
        Cmd::perform(fetch)
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event.as_key_press()?.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Loaded(value) => {
                self.status = Status::Ready(value);
                Cmd::none()
            }
            Msg::Failed => {
                self.status = Status::Retrying;
                self.attempts += 1;
                // A scheduled one-shot timer: real wait under `run_threaded`,
                // zero virtual delay under the inline Harness/`run`.
                Cmd::tick(std::time::Duration::from_millis(20), || Msg::Retry)
            }
            Msg::Retry => {
                self.status = Status::Loading;
                Cmd::perform(fetch)
            }
            Msg::Quit => Cmd::quit(),
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let line = match self.status {
            Status::Loading => "loading…".to_string(),
            Status::Retrying => format!("retry #{}…", self.attempts),
            Status::Ready(value) => format!("value: {value}"),
        };
        let pos = frame.area().position();
        frame.buffer_mut().set_str(pos, &line, Style::new());
    }
}

fn main() {
    FETCH_SUCCEEDS.store(0, Ordering::Relaxed);
    // Harness ⇒ inline executor: `init`'s perform "fails" immediately, the
    // retry `tick` fires immediately, the second fetch succeeds — the whole
    // async-looking flow settles before the first frame, deterministically.
    let mut harness = Harness::new(Dashboard::default(), 12, 1);
    println!("after init -> {}", harness.snapshot().trim_end());
    assert!(
        matches!(harness.app().status, Status::Ready(42)),
        "inline executor settles perform→fail→tick→retry→perform→ready at once",
    );
    assert_eq!(harness.app().attempts, 1, "exactly one retry happened");
    assert_eq!(harness.snapshot(), "value: 42   \n");

    harness.handle(Event::from(KeyEvent::char('q')));
    assert!(!harness.is_running(), "q quits");
    println!("final     -> {}", harness.snapshot().trim_end());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole perform→fail→tick→retry→perform→ready flow settles before the
    /// first frame under the inline executor, with no clock — the deterministic
    /// guarantee. (`run_threaded`'s off-loop behavior is covered authoritatively
    /// by the `run` module's threaded tests; here the point is that the
    /// *identical* app is fully assertable headless.)
    #[test]
    fn inline_harness_settles_the_whole_async_flow_deterministically() {
        FETCH_SUCCEEDS.store(0, Ordering::Relaxed);
        let harness = Harness::new(Dashboard::default(), 12, 1);
        assert!(matches!(harness.app().status, Status::Ready(42)));
        assert_eq!(harness.app().attempts, 1);
        assert_eq!(harness.snapshot(), "value: 42   \n");
    }
}
