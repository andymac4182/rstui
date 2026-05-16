//! The contract every rstui application implements: Elm's
//! model/update/view, adapted to idiomatic Rust.
//!
//! An [`App`] owns its state and exposes three pure-ish seams:
//!
//! - [`on_event`](App::on_event) translates a terminal [`Event`] into an
//!   optional application message. It takes `&self`: input handling decides
//!   *intent*, it never mutates state.
//! - [`update`](App::update) folds one message into the state and returns a
//!   [`Cmd`] for any follow-up work. This is the **single** place state
//!   changes, which is what makes an app replayable and unit-testable.
//! - [`view`](App::view) renders the current state into a [`Frame`]. It takes
//!   `&self`: rendering never mutates state.
//!
//! [`init`](App::init) supplies an optional startup command (Bubble Tea's
//! `Init`) so an app can kick off work — load data, start a tick — before the
//! first frame.
//!
//! This split is deliberately stricter than Bubble Tea (which folds raw input
//! through `Update`): funnelling every mutation through `update` keeps the
//! reducer the one testable source of truth and leaves room for a future
//! focus router to dispatch events to a focused component *before* the app's
//! `on_event` ever sees them.
//!
//! # Example
//!
//! A complete counter app. Drive it with a [`Harness`](crate::Harness) to test
//! it with no terminal:
//!
//! ```
//! use rstui_runtime::{App, Cmd, Event, Frame, Harness};
//! use rstui_core::{KeyCode, Style};
//!
//! #[derive(Default)]
//! struct Counter {
//!     value: i64,
//! }
//!
//! enum Msg {
//!     Increment,
//!     Quit,
//! }
//!
//! impl App for Counter {
//!     type Message = Msg;
//!
//!     fn on_event(&self, event: Event) -> Option<Msg> {
//!         let key = event.as_key_press()?;
//!         match key.code {
//!             KeyCode::Char('+') => Some(Msg::Increment),
//!             KeyCode::Char('q') => Some(Msg::Quit),
//!             _ => None,
//!         }
//!     }
//!
//!     fn update(&mut self, message: Msg) -> Cmd<Msg> {
//!         match message {
//!             Msg::Increment => {
//!                 self.value += 1;
//!                 Cmd::none()
//!             }
//!             Msg::Quit => Cmd::quit(),
//!         }
//!     }
//!
//!     fn view(&self, frame: &mut Frame<'_>) {
//!         let pos = frame.area().position();
//!         frame
//!             .buffer_mut()
//!             .set_str(pos, &format!("count: {}", self.value), Style::new());
//!     }
//! }
//!
//! let mut harness = Harness::new(Counter::default(), 10, 1);
//! harness.handle(Event::from(rstui_core::KeyEvent::char('+')));
//! assert_eq!(harness.app().value, 1);
//! assert_eq!(harness.snapshot(), "count: 1  \n");
//! ```

use std::time::Duration;

use rstui_core::{Event, Frame};

use crate::cmd::Cmd;

/// An rstui application: state plus the update/view/event seams the runtime
/// drives.
///
/// Implement this on your state type. The runtime (or a
/// [`Harness`](crate::Harness)) owns the value and calls these methods; you
/// never call them yourself, which is what keeps the data flow one-directional
/// and testable.
pub trait App {
    /// The message type [`update`](App::update) folds into the state.
    ///
    /// Almost always an `enum` of everything that can happen in the app —
    /// user intents from [`on_event`](App::on_event) and results delivered by
    /// [`Cmd`]s.
    type Message;

    /// The command to run once, before the first frame.
    ///
    /// Defaults to [`Cmd::none`]. Override to start work the UI depends on
    /// (an initial load, a recurring tick) the moment the app starts.
    fn init(&mut self) -> Cmd<Self::Message> {
        Cmd::none()
    }

    /// Translates a terminal [`Event`] into an application message, or `None`
    /// to ignore it.
    ///
    /// This is where keymaps live. It takes `&self` on purpose: deciding what
    /// an input *means* may depend on state (a modal vs. normal mode) but must
    /// not change it — every mutation goes through [`update`](App::update).
    ///
    /// Defaults to ignoring all input, for apps driven only by [`Cmd`]s.
    fn on_event(&self, event: Event) -> Option<Self::Message> {
        let _ = event;
        None
    }

    /// How often the runtime should wake itself to [`tick`](App::on_tick) when
    /// no input is arriving, or [`None`] to only ever wake on input.
    ///
    /// This is the one seam that turns the otherwise input-driven loop into an
    /// animation/clock loop: a spinner, a countdown, a progress poll. It takes
    /// `&self`, so the cadence is a pure function of state — return
    /// `Some(Duration::from_millis(80))` while an animation is running and
    /// `None` when it finishes and the loop should go back to blocking purely
    /// on input. Defaulting to `None` keeps every input-only app behaving
    /// exactly as before (the live loop blocks on input with no clock at all).
    ///
    /// The cadence is steady wall-clock pacing in the live loop, *independent*
    /// of input: an event arriving does not reset the tick clock, so an
    /// animation stays smooth under typing. Missed ticks coalesce (a slow frame
    /// never schedules a catch-up storm). The headless
    /// [`Harness`](crate::Harness) ignores this entirely and instead exposes an
    /// explicit [`Harness::tick`](crate::Harness::tick) so tests advance time
    /// deterministically with no real clock — the shared command-settling core
    /// is unchanged, so a ticking app is as testable as any other.
    fn tick_rate(&self) -> Option<Duration> {
        None
    }

    /// Translates a periodic timer wake into an application message, or [`None`]
    /// to let the wake pass without changing state.
    ///
    /// The temporal dual of [`on_event`](App::on_event): `on_event` maps
    /// *input* to intent, `on_tick` maps the *passage of time* to intent (the
    /// next animation frame, a re-poll). It takes `&self` for the same reason —
    /// deciding what a tick *means* may depend on state but must not change it;
    /// every mutation still goes through [`update`](App::update). A tick is
    /// deliberately **not** an [`Event`] variant: terminal input and runtime
    /// timing are different concepts and never share a type.
    ///
    /// Only consulted when [`tick_rate`](App::tick_rate) is `Some`; the default
    /// ignores ticks, which is why an app that overrides neither is byte-for-
    /// byte the old input-only loop.
    fn on_tick(&self) -> Option<Self::Message> {
        None
    }

    /// Folds one message into the state and returns any follow-up work.
    ///
    /// The only place the app mutates. Return [`Cmd::none`] when nothing else
    /// needs to happen, [`Cmd::quit`] to stop the program, or
    /// [`Cmd::perform`]/[`Cmd::batch`] to schedule effects whose results come
    /// back as more messages.
    fn update(&mut self, message: Self::Message) -> Cmd<Self::Message>;

    /// Renders the current state into `frame`.
    ///
    /// Pure with respect to app state (`&self`). The frame always starts blank,
    /// so a view just describes what the screen should show now; the runtime
    /// diffs it and sends only what changed.
    fn view(&self, frame: &mut Frame<'_>);
}
