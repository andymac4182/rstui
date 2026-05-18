//! A deterministic, terminal-free driver for an [`App`].
//!
//! [`Harness`] is to the runtime what [`TestBackend`] is to a backend: it runs
//! the real loop — `init`, `on_event` → `update`, command draining, `view` —
//! against an in-memory [`TestBackend`], with no TTY, threads, or wall clock.
//! Feed it events or messages, then assert on the app state and the rendered
//! [`snapshot`](Harness::snapshot).
//!
//! It is also the reference semantics for the future real runtime: same
//! ordering, same command-settling rule, just synchronous so tests are exact.
//!
//! ## Command settling
//!
//! After every input the harness processes commands to a fixed point: a
//! [`perform`](crate::Cmd::perform) message re-enters [`update`](App::update),
//! whose returned command is processed too, breadth-first and in order, until
//! no work remains. A [`quit`](crate::Cmd::quit) stops the program; further
//! input is
//! then ignored. A pathological app that produces messages without end is
//! bounded by [`command_budget`](Harness::with_command_budget) and panics
//! rather than hanging a test.
//!
//! # Example
//!
//! ```
//! use rstui_runtime::{App, Cmd, Event, Frame, Harness};
//! use rstui_core::Style;
//!
//! struct Echo {
//!     ticks: u32,
//! }
//!
//! enum Msg {
//!     Tick,
//! }
//!
//! impl App for Echo {
//!     type Message = Msg;
//!
//!     // Startup work: schedule one tick. Its result re-enters `update`.
//!     fn init(&mut self) -> Cmd<Msg> {
//!         Cmd::perform(|| Msg::Tick)
//!     }
//!
//!     fn update(&mut self, _: Msg) -> Cmd<Msg> {
//!         self.ticks += 1;
//!         Cmd::none()
//!     }
//!
//!     fn view(&self, frame: &mut Frame<'_>) {
//!         let pos = frame.area().position();
//!         frame
//!             .buffer_mut()
//!             .set_str(pos, &format!("ticks: {}", self.ticks), Style::new());
//!     }
//! }
//!
//! // `new` runs `init` and settles its command before the first frame.
//! let mut harness = Harness::new(Echo { ticks: 0 }, 10, 1);
//! assert_eq!(harness.app().ticks, 1);
//! assert_eq!(harness.snapshot(), "ticks: 1  \n");
//! assert!(harness.is_running());
//! ```

use std::time::{Duration, Instant};

use rstui_core::{Event, Terminal, TestBackend};

use crate::app::App;
use crate::cmd::InlineExecutor;
use crate::run::{DEFAULT_COMMAND_BUDGET, FrameMetrics, Settled, settle};

/// Drives an [`App`] over an in-memory [`TestBackend`] with no terminal.
///
/// Construct with [`Harness::new`], drive it with [`handle`](Harness::handle),
/// [`message`](Harness::message), [`resize`](Harness::resize), or
/// [`tick`](Harness::tick) (an explicit, clock-free elapsed-timer step), then
/// assert on [`app`](Harness::app) and [`snapshot`](Harness::snapshot).
#[derive(Debug)]
pub struct Harness<A: App> {
    app: A,
    terminal: Terminal<TestBackend>,
    running: bool,
    command_budget: usize,
    /// ADR 0018 §3: the last driven iteration's metrics, for deterministic
    /// headless perf testing. `None` until the first
    /// `handle`/`message`/`tick` (the initial `new` render has no `logic`
    /// phase to attribute).
    last_frame: Option<FrameMetrics>,
}

impl<A: App> Harness<A> {
    /// Creates a harness for `app` on a `width` × `height` surface.
    ///
    /// Runs [`App::init`], settles the command it returns, then renders the
    /// first frame — so [`snapshot`](Harness::snapshot) is meaningful
    /// immediately and an `init` that quits is observed.
    pub fn new(app: A, width: u16, height: u16) -> Self {
        // TestBackend is infallible, so terminal construction cannot fail.
        let terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend is infallible");
        let mut harness = Self {
            app,
            terminal,
            running: true,
            command_budget: DEFAULT_COMMAND_BUDGET,
            last_frame: None,
        };
        let cmd = harness.app.init();
        harness.settle(cmd);
        harness.render();
        harness
    }

    /// Sets how many `update`/`perform` steps a single input may produce
    /// before the harness panics. See [the module docs](self#command-settling).
    #[must_use]
    pub fn with_command_budget(mut self, budget: usize) -> Self {
        self.command_budget = budget;
        self
    }

    /// Delivers `event` to the app: [`on_event`](App::on_event), then
    /// [`update`](App::update) for any message it produces, then a re-render.
    ///
    /// A no-op once the app has quit. The re-render happens even when no
    /// message is produced so a resize repaint still occurs.
    pub fn handle(&mut self, event: Event) {
        if !self.running {
            return;
        }
        let t0 = Instant::now();
        if let Some(message) = self.app.on_event(event) {
            let cmd = self.app.update(message);
            self.settle(cmd);
        }
        let logic = t0.elapsed();
        self.record_render(logic, 1);
    }

    /// Injects `message` straight into [`update`](App::update), bypassing
    /// [`on_event`](App::on_event), then re-renders.
    ///
    /// Useful for testing reducer logic and command feedback without crafting
    /// the input event that would produce the message. A no-op once quit.
    pub fn message(&mut self, message: A::Message) {
        if !self.running {
            return;
        }
        let t0 = Instant::now();
        let cmd = self.app.update(message);
        self.settle(cmd);
        let logic = t0.elapsed();
        self.record_render(logic, 0);
    }

    /// Resizes the surface to `width` × `height`, delivers a matching
    /// [`Event::Resize`] to the app, and repaints the full screen.
    ///
    /// Mirrors how a real backend reports a terminal resize: the surface
    /// changes *and* the app gets a chance to react.
    pub fn resize(&mut self, width: u16, height: u16) {
        if !self.running {
            return;
        }
        self.terminal.backend_mut().resize(width, height);
        // `Terminal::draw` autoresizes from the backend and forces a full
        // repaint; routing the event through `handle` also lets the app react.
        self.handle(Event::Resize(rstui_core::Size::new(width, height)));
    }

    /// Delivers one elapsed timer tick to the app: [`on_tick`](App::on_tick),
    /// then [`update`](App::update) for any message it produces, then a
    /// re-render — the deterministic twin of the live loop's
    /// [`tick_rate`](App::tick_rate) wake.
    ///
    /// The live [`run`](crate::run()) loop calls this same `on_tick` → `update`
    /// → `settle` → render path when a real timer elapses; the harness exposes
    /// it as an explicit step so a test advances time **by calling `tick`**,
    /// with no wall clock. One `tick()` is exactly one elapsed period: assert a
    /// spinner advanced one frame, a countdown decremented once. A no-op once
    /// the app has quit, and (like a real idle tick) it still re-renders even
    /// when `on_tick` produced no message.
    ///
    /// [`tick_rate`](App::tick_rate) itself is never consulted here — cadence
    /// is the live loop's concern; determinism means the *test* decides when
    /// ticks happen. Read it directly via [`app`](Harness::app) if a test wants
    /// to assert the app's declared cadence.
    pub fn tick(&mut self) {
        if !self.running {
            return;
        }
        let t0 = Instant::now();
        if let Some(message) = self.app.on_tick() {
            let cmd = self.app.update(message);
            self.settle(cmd);
        }
        let logic = t0.elapsed();
        self.record_render(logic, 0);
    }

    /// A shared reference to the app, for asserting on its state.
    #[must_use]
    pub fn app(&self) -> &A {
        &self.app
    }

    /// Whether the app is still running (no [`Cmd::quit`](crate::Cmd::quit) has
    /// settled).
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// The in-memory backend, to assert on cells or the cursor directly.
    #[must_use]
    pub fn backend(&self) -> &TestBackend {
        self.terminal.backend()
    }

    /// The rendered screen as a deterministic, newline-terminated string —
    /// the snapshot to assert against.
    #[must_use]
    pub fn snapshot(&self) -> String {
        format!("{}", self.terminal.backend())
    }

    /// Processes a command and every message it cascades into, in order, until
    /// the work settles or the budget is exceeded.
    ///
    /// Delegates to the *exact* [`settle`](crate::run::settle) state machine
    /// the live [`run`](crate::run()) loop uses, with an
    /// [`InlineExecutor`](crate::cmd::InlineExecutor): every
    /// [`perform`](crate::Cmd::perform)/[`tick`](crate::Cmd::tick) runs now
    /// with zero virtual delay, so the harness stays clock-free and
    /// deterministic and cannot drift from production — it is that loop with a
    /// [`TestBackend`] and scripted input swapped in.
    fn settle(&mut self, cmd: crate::Cmd<A::Message>) {
        if settle(&mut self.app, cmd, self.command_budget, &mut InlineExecutor) == Settled::Quit {
            self.running = false;
        }
    }

    /// Renders the current app state into a fresh frame and presents it.
    fn render(&mut self) {
        // Split borrow: the view reads `self.app` while `self.terminal` draws.
        let app = &self.app;
        self.terminal
            .draw(|frame| app.view(frame))
            .expect("TestBackend is infallible");
    }

    /// Renders like [`render`](Self::render) but splits `view`/`flush` and
    /// stores a [`FrameMetrics`] in `last_frame` — the deterministic twin
    /// of the live loop's observed path (ADR 0018 §3).
    fn record_render(&mut self, logic: Duration, events_coalesced: u32) {
        let app = &self.app;
        let mut view = Duration::ZERO;
        let t0 = Instant::now();
        self.terminal
            .draw(|frame| {
                let v = Instant::now();
                app.view(frame);
                view = v.elapsed();
            })
            .expect("TestBackend is infallible");
        let draw = t0.elapsed();
        let flush = draw.saturating_sub(view);
        let total = logic + draw;
        let frame = self.last_frame.map_or(0, |m| m.frame + 1);
        self.last_frame = Some(FrameMetrics {
            frame,
            logic,
            view,
            flush,
            total,
            produced: true,
            events_coalesced,
            input_latency: total,
        });
    }

    /// The last `handle`/`message`/`tick` iteration's [`FrameMetrics`], or
    /// `None` before the first one. The headless, deterministic mirror of
    /// the live loop's [`FrameObserver`](crate::FrameObserver) — drive the
    /// harness, then assert on phase durations / `produced` in a test.
    #[must_use]
    pub fn last_frame(&self) -> Option<&FrameMetrics> {
        self.last_frame.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cmd;
    use rstui_core::{KeyCode, KeyEvent, Style};

    /// A counter that increments on `+`, schedules a deferred bump, and quits
    /// on `q` — exercises events, reducer state, command feedback, and quit.
    #[derive(Default)]
    struct Counter {
        value: i64,
    }

    enum Msg {
        Inc,
        /// Increment, then schedule one more increment via a command.
        IncThenDefer,
        Quit,
    }

    impl App for Counter {
        type Message = Msg;

        fn on_event(&self, event: Event) -> Option<Msg> {
            let key = event.as_key_press()?;
            match key.code {
                KeyCode::Char('+') => Some(Msg::Inc),
                KeyCode::Char('d') => Some(Msg::IncThenDefer),
                KeyCode::Char('q') => Some(Msg::Quit),
                _ => None,
            }
        }

        fn update(&mut self, message: Msg) -> Cmd<Msg> {
            match message {
                Msg::Inc => {
                    self.value += 1;
                    Cmd::none()
                }
                Msg::IncThenDefer => {
                    self.value += 1;
                    Cmd::perform(|| Msg::Inc)
                }
                Msg::Quit => Cmd::quit(),
            }
        }

        fn view(&self, frame: &mut rstui_core::Frame<'_>) {
            let pos = frame.area().position();
            frame
                .buffer_mut()
                .set_str(pos, &format!("n={}", self.value), Style::new());
        }
    }

    #[test]
    fn new_renders_the_initial_frame() {
        let harness = Harness::new(Counter::default(), 6, 1);
        assert_eq!(harness.app().value, 0);
        assert_eq!(harness.snapshot(), "n=0   \n");
        assert!(harness.is_running());
    }

    #[test]
    fn handle_maps_an_event_through_on_event_and_update() {
        let mut harness = Harness::new(Counter::default(), 6, 1);
        harness.handle(Event::from(KeyEvent::char('+')));
        assert_eq!(harness.app().value, 1);
        assert_eq!(harness.snapshot(), "n=1   \n");

        // An unmapped key produces no message and leaves state untouched.
        harness.handle(Event::from(KeyEvent::char('z')));
        assert_eq!(harness.app().value, 1);
    }

    #[test]
    fn message_bypasses_on_event() {
        let mut harness = Harness::new(Counter::default(), 6, 1);
        harness.message(Msg::Inc);
        harness.message(Msg::Inc);
        assert_eq!(harness.app().value, 2);
        assert_eq!(harness.snapshot(), "n=2   \n");
    }

    #[test]
    fn command_results_re_enter_update() {
        let mut harness = Harness::new(Counter::default(), 6, 1);
        // IncThenDefer bumps once and a Cmd feeds another Inc back in.
        harness.message(Msg::IncThenDefer);
        assert_eq!(harness.app().value, 2);
        assert_eq!(harness.snapshot(), "n=2   \n");
    }

    #[test]
    fn quit_stops_the_app_and_freezes_further_input() {
        let mut harness = Harness::new(Counter::default(), 6, 1);
        harness.handle(Event::from(KeyEvent::char('+')));
        harness.handle(Event::from(KeyEvent::char('q')));
        assert!(!harness.is_running());

        // Input after quit is ignored; the last rendered frame stands.
        harness.handle(Event::from(KeyEvent::char('+')));
        harness.message(Msg::Inc);
        assert_eq!(harness.app().value, 1);
        assert_eq!(harness.snapshot(), "n=1   \n");
    }

    #[test]
    fn resize_grows_the_surface_and_repaints() {
        let mut harness = Harness::new(Counter::default(), 3, 1);
        assert_eq!(harness.snapshot(), "n=0\n");
        harness.resize(6, 2);
        assert_eq!(harness.snapshot(), "n=0   \n      \n");
    }

    /// An app whose every message schedules another — must hit the budget.
    struct Runaway;

    impl App for Runaway {
        type Message = ();

        fn update(&mut self, (): ()) -> Cmd<()> {
            Cmd::perform(|| ())
        }

        fn view(&self, _: &mut rstui_core::Frame<'_>) {}
    }

    #[test]
    #[should_panic(expected = "command loop exceeded")]
    fn unbounded_command_cycle_panics_at_the_budget() {
        let mut harness = Harness::new(Runaway, 1, 1).with_command_budget(16);
        harness.message(());
    }

    #[test]
    fn init_command_is_settled_before_the_first_frame() {
        struct Boot;
        impl App for Boot {
            type Message = ();
            fn init(&mut self) -> Cmd<()> {
                Cmd::quit()
            }
            fn update(&mut self, (): ()) -> Cmd<()> {
                Cmd::none()
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        // init() returned quit, so the app is not running after construction.
        let harness = Harness::new(Boot, 2, 1);
        assert!(!harness.is_running());
    }

    /// A spinner whose frame advances on each tick while animating and stops
    /// (drops its tick rate) on `s` — exercises `tick_rate`/`on_tick` and the
    /// reducer turning ticking off by state.
    struct Spinner {
        frame: usize,
        animating: bool,
    }

    enum SpinMsg {
        Advance,
        Stop,
    }

    impl App for Spinner {
        type Message = SpinMsg;

        fn tick_rate(&self) -> Option<std::time::Duration> {
            self.animating.then(|| std::time::Duration::from_millis(80))
        }

        fn on_tick(&self) -> Option<SpinMsg> {
            self.animating.then_some(SpinMsg::Advance)
        }

        fn on_event(&self, event: Event) -> Option<SpinMsg> {
            match event.as_key_press()?.code {
                KeyCode::Char('s') => Some(SpinMsg::Stop),
                _ => None,
            }
        }

        fn update(&mut self, message: SpinMsg) -> Cmd<SpinMsg> {
            match message {
                SpinMsg::Advance => {
                    self.frame += 1;
                    Cmd::none()
                }
                SpinMsg::Stop => {
                    self.animating = false;
                    Cmd::none()
                }
            }
        }

        fn view(&self, frame: &mut rstui_core::Frame<'_>) {
            let pos = frame.area().position();
            frame
                .buffer_mut()
                .set_str(pos, &format!("f={}", self.frame), Style::new());
        }
    }

    #[test]
    fn tick_advances_state_deterministically_with_no_clock() {
        let mut harness = Harness::new(
            Spinner {
                frame: 0,
                animating: true,
            },
            4,
            1,
        );
        assert_eq!(harness.snapshot(), "f=0 \n");
        // Each explicit tick is exactly one elapsed period — no wall clock.
        harness.tick();
        assert_eq!(harness.app().frame, 1);
        harness.tick();
        harness.tick();
        assert_eq!(harness.app().frame, 3);
        // It re-rendered on every tick, like a real idle wake.
        assert_eq!(harness.snapshot(), "f=3 \n");
        // The declared cadence is readable through the app for assertions.
        assert_eq!(
            harness.app().tick_rate(),
            Some(std::time::Duration::from_millis(80))
        );
    }

    #[test]
    fn an_event_can_stop_ticking_and_further_ticks_are_inert() {
        let mut harness = Harness::new(
            Spinner {
                frame: 0,
                animating: true,
            },
            4,
            1,
        );
        harness.tick();
        assert_eq!(harness.app().frame, 1);

        // `s` flips animating off: tick_rate becomes None and on_tick stops
        // producing messages, so subsequent ticks change nothing.
        harness.handle(Event::from(KeyEvent::char('s')));
        assert_eq!(harness.app().tick_rate(), None);
        harness.tick();
        harness.tick();
        assert_eq!(harness.app().frame, 1, "ticks are inert once stopped");
    }

    #[test]
    fn tick_is_a_noop_after_quit_and_renders_when_on_tick_is_silent() {
        // `Counter` overrides neither tick_rate nor on_tick (both default to
        // None), so `tick()` must be a pure re-render — proving the feature is
        // strictly opt-in and existing apps are unaffected.
        let mut harness = Harness::new(Counter::default(), 6, 1);
        harness.message(Msg::Inc);
        assert_eq!(harness.snapshot(), "n=1   \n");
        harness.tick(); // on_tick() == None: state untouched, still re-renders
        assert_eq!(harness.app().value, 1);
        assert_eq!(harness.snapshot(), "n=1   \n");

        // After quit, tick is a no-op like handle/message.
        harness.handle(Event::from(KeyEvent::char('q')));
        assert!(!harness.is_running());
        harness.tick();
        assert_eq!(harness.app().value, 1);
    }

    // ADR 0018 §3: the headless FrameObserver mirror. Structural asserts
    // only (relationships, counts, the monotonic frame index) — never
    // absolute wall times, so the test is deterministic.
    #[test]
    fn last_frame_records_observed_metrics_for_every_drive() {
        let mut h = Harness::new(Counter::default(), 6, 1);
        // None until the first drive — the init render has no `logic`
        // phase to attribute.
        assert!(h.last_frame().is_none());

        h.handle(Event::from(KeyEvent::char('+')));
        let f0 = *h.last_frame().expect("recorded after handle");
        assert_eq!(f0.frame, 0);
        assert!(f0.produced);
        assert_eq!(f0.events_coalesced, 1); // one input event
        assert!(f0.total >= f0.view + f0.flush);
        assert!(f0.total >= f0.logic);

        h.message(Msg::Inc);
        let f1 = *h.last_frame().unwrap();
        assert_eq!(f1.frame, 1); // monotonic across drive kinds
        assert_eq!(f1.events_coalesced, 0); // a direct message — no input event
        assert!(f1.produced);

        h.tick();
        let f2 = *h.last_frame().unwrap();
        assert_eq!(f2.frame, 2);
        assert_eq!(f2.events_coalesced, 0);

        // The quit iteration still renders+records (the live loop paints
        // the final frame too); a *subsequent* drive is a no-op and must
        // not advance the recorded frame.
        h.handle(Event::from(KeyEvent::char('q')));
        assert!(!h.is_running());
        let quit_frame = h.last_frame().unwrap().frame;
        assert_eq!(quit_frame, 3);
        h.handle(Event::from(KeyEvent::char('+')));
        assert_eq!(h.last_frame().unwrap().frame, quit_frame); // unchanged
    }
}
