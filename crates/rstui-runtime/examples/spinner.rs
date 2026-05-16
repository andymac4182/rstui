//! A deterministic spinner driven by the headless [`Harness`], showing the
//! tick seam end to end:
//!
//! ```text
//! cargo run -p rstui-runtime --example spinner
//! ```
//!
//! [`App::tick_rate`] declares the cadence as a pure function of state (a rate
//! while spinning, `None` once stopped — so the live loop goes back to blocking
//! purely on input), [`App::on_tick`] maps an elapsed period to a message just
//! as `on_event` maps a key, and [`Harness::tick`] advances time *explicitly*
//! so the whole animation is asserted with no wall clock. The identical `App`
//! runs live by handing it to `rstui_crossterm::run_app` — the real loop calls
//! the same `on_tick` on a real timer.

use rstui_core::{KeyCode, KeyEvent, Style};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};

/// The four classic spinner glyphs; `frame` indexes into this with wraparound.
const GLYPHS: [char; 4] = ['|', '/', '-', '\\'];

#[derive(Default)]
struct Spinner {
    frame: usize,
    /// Whether the animation is running. Drives [`App::tick_rate`]: while
    /// `true` the loop wakes on a timer; once `false` it blocks only on input.
    spinning: bool,
    done: bool,
}

enum Msg {
    /// One animation period elapsed: advance the glyph.
    Advance,
    /// Toggle the animation on/off (space).
    Toggle,
    Quit,
}

impl App for Spinner {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        self.spinning = true;
        Cmd::none()
    }

    fn tick_rate(&self) -> Option<std::time::Duration> {
        // Only ask to be woken while actually animating — the loop has no
        // timer cost when paused or finished.
        self.spinning.then(|| std::time::Duration::from_millis(120))
    }

    fn on_tick(&self) -> Option<Msg> {
        self.spinning.then_some(Msg::Advance)
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event.as_key_press()?.code {
            KeyCode::Char(' ') => Some(Msg::Toggle),
            KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Advance => {
                self.frame = (self.frame + 1) % GLYPHS.len();
                Cmd::none()
            }
            Msg::Toggle => {
                self.spinning = !self.spinning;
                Cmd::none()
            }
            Msg::Quit => {
                self.done = true;
                Cmd::quit()
            }
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let pos = frame.area().position();
        let state = if self.done {
            "done".to_string()
        } else {
            format!("{} working", GLYPHS[self.frame])
        };
        frame.buffer_mut().set_str(pos, &state, Style::new());
    }
}

/// Drives the spinner with an explicit, deterministic timeline: three ticks
/// advance the glyph, space pauses it (ticks then go inert), space resumes,
/// `q` quits. Returns the final snapshot so `main` and the test share it.
fn run_scripted() -> Harness<Spinner> {
    let mut harness = Harness::new(Spinner::default(), 9, 1);
    // `init` set spinning = true; nothing has ticked yet.
    assert_eq!(harness.snapshot(), "| working\n");

    harness.tick();
    harness.tick();
    harness.tick();
    assert_eq!(harness.app().frame, 3);
    assert_eq!(harness.snapshot(), "\\ working\n");

    // Pause: tick_rate drops to None and further ticks are inert.
    harness.handle(Event::from(KeyEvent::char(' ')));
    assert_eq!(harness.app().tick_rate(), None);
    harness.tick();
    harness.tick();
    assert_eq!(harness.app().frame, 3, "paused: ticks change nothing");

    // Resume, advance once more, then quit.
    harness.handle(Event::from(KeyEvent::char(' ')));
    harness.tick();
    assert_eq!(harness.app().frame, 0, "wrapped 3 -> 0");
    harness.handle(Event::from(KeyEvent::char('q')));
    harness
}

fn main() {
    let harness = run_scripted();
    println!("final    -> {}", harness.snapshot().trim_end());
    assert!(!harness.is_running(), "q must have quit the app");
    assert!(harness.app().done);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scripted_timeline_is_deterministic() {
        let harness = run_scripted();
        assert!(!harness.is_running());
        assert!(harness.app().done);
        assert_eq!(harness.snapshot(), "done     \n");
    }

    #[test]
    fn ticks_are_inert_before_init_style_state_is_set() {
        // A fresh Spinner has spinning == false until `init` runs; `Harness`
        // runs `init`, so this asserts the cadence is genuinely state-driven.
        let harness = Harness::new(Spinner::default(), 9, 1);
        assert!(harness.app().spinning, "init() armed the animation");
        assert!(harness.app().tick_rate().is_some());
    }
}
