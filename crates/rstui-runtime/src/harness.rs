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
//! [`perform`](Cmd::perform) message re-enters [`update`](App::update), whose
//! returned command is processed too, breadth-first and in order, until no
//! work remains. A [`quit`](Cmd::quit) stops the program; further input is
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

use rstui_core::{Event, Terminal, TestBackend};

use crate::app::App;
use crate::run::{DEFAULT_COMMAND_BUDGET, Settled, settle};

/// Drives an [`App`] over an in-memory [`TestBackend`] with no terminal.
///
/// Construct with [`Harness::new`], drive it with [`handle`](Harness::handle),
/// [`message`](Harness::message), or [`resize`](Harness::resize), then assert
/// on [`app`](Harness::app) and [`snapshot`](Harness::snapshot).
#[derive(Debug)]
pub struct Harness<A: App> {
    app: A,
    terminal: Terminal<TestBackend>,
    running: bool,
    command_budget: usize,
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
        if let Some(message) = self.app.on_event(event) {
            let cmd = self.app.update(message);
            self.settle(cmd);
        }
        self.render();
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
        let cmd = self.app.update(message);
        self.settle(cmd);
        self.render();
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

    /// A shared reference to the app, for asserting on its state.
    #[must_use]
    pub fn app(&self) -> &A {
        &self.app
    }

    /// Whether the app is still running (no [`Cmd::quit`] has settled).
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
    /// the live [`run`](crate::run) loop uses, so the harness's semantics
    /// cannot drift from production: the harness is that loop with a
    /// [`TestBackend`] and scripted input swapped in.
    fn settle(&mut self, cmd: crate::Cmd<A::Message>) {
        if settle(&mut self.app, cmd, self.command_budget) == Settled::Quit {
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
}
