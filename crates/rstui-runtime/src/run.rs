//! The **live** event loop: the production twin of [`Harness`](crate::Harness).
//!
//! [`run`] drives an [`App`] over a real [`Backend`] and [`EventSource`] with
//! the *exact same* ordering and command-settling rule the headless
//! [`Harness`](crate::Harness) uses — because they call the same
//! [`settle`] state machine. The harness is therefore not merely *a*
//! reference for the live loop; it is literally the same reducer logic with a
//! [`TestBackend`](rstui_core::TestBackend) and a scripted
//! [`TestEventSource`](rstui_core::TestEventSource) swapped in for the real
//! terminal. That is what makes "the same `App`/`Cmd` code runs headless in
//! tests and live on a terminal, unchanged" true *by construction* rather than
//! by hopeful comment.
//!
//! The flow mirrors [`Harness`](crate::Harness) precisely:
//!
//! 1. [`init`](App::init), settle its command, render the first frame (so the
//!    screen is correct before any input).
//! 2. Block on [`poll_event(None)`](rstui_core::EventSource::poll_event) for
//!    the next input. Map it through [`on_event`](App::on_event) →
//!    [`update`](App::update), settle any follow-up commands, then repaint
//!    (a no-op cell diff if nothing changed, so an unhandled key is cheap).
//! 3. A [`Cmd::quit`](crate::Cmd::quit) stops the loop. So does end-of-input:
//!    `poll_event(None)` returning `Ok(None)` means input is *permanently*
//!    exhausted (the terminal closed, or a scripted source drained).
//!
//! ## What this slice deliberately does not do
//!
//! - **Commands run inline**, exactly as the harness runs them — a slow
//!   [`Cmd::perform`](crate::Cmd::perform) blocks the loop. Offloading work to
//!   a thread pool / async executor is the reason the closure is already
//!   `Send + 'static`; it is a separable future slice, not a stub.
//! - **No periodic tick.** The loop blocks on input
//!   ([`poll_event(None)`](rstui_core::EventSource::poll_event)); the
//!   `Some(timeout)` branch of the [`EventSource`] contract is reserved for a
//!   future animation/tick slice. Deferred, not stubbed.
//! - **No panic hook.** Restoring the terminal on a panic is the backend
//!   guard's `Drop` (proven for the crossterm guard); making the panic
//!   *message* visible afterwards belongs with runtime panic policy, a
//!   distinct concern.
//!
//! # Example
//!
//! The same counter the [`Harness`](crate::Harness) tests exercise, driven
//! here by the live loop over a scripted source — no TTY. Swap in a crossterm
//! backend and input source and the identical [`run`] call runs on a real
//! terminal.
//!
//! ```
//! use rstui_runtime::{App, Cmd, Event, Frame, run};
//! use rstui_core::{KeyCode, KeyEvent, Style, TestBackend, TestEventSource};
//!
//! #[derive(Default)]
//! struct Counter {
//!     value: i64,
//! }
//!
//! enum Msg {
//!     Inc,
//!     Quit,
//! }
//!
//! impl App for Counter {
//!     type Message = Msg;
//!
//!     fn on_event(&self, event: Event) -> Option<Msg> {
//!         match event.as_key_press()?.code {
//!             KeyCode::Char('+') => Some(Msg::Inc),
//!             KeyCode::Char('q') => Some(Msg::Quit),
//!             _ => None,
//!         }
//!     }
//!
//!     fn update(&mut self, message: Msg) -> Cmd<Msg> {
//!         match message {
//!             Msg::Inc => {
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
//!             .set_str(pos, &format!("n={}", self.value), Style::new());
//!     }
//! }
//!
//! let mut input = TestEventSource::with_events([
//!     Event::from(KeyEvent::char('+')),
//!     Event::from(KeyEvent::char('+')),
//!     Event::from(KeyEvent::char('q')),
//! ]);
//! let final_app = run(Counter::default(), TestBackend::new(8, 1), &mut input).unwrap();
//! assert_eq!(final_app.value, 2);
//! ```

use std::collections::VecDeque;
use std::fmt;

use rstui_core::{Backend, EventSource, Terminal};

use crate::app::App;
use crate::cmd::Cmd;

/// The default cap on `update`/`perform` steps a single input may produce
/// before the command loop gives up. Generous enough for real cascades, low
/// enough to fail a runaway reducer fast.
///
/// This bounds the shared [`settle`] state machine, so it governs both the
/// live [`run`] loop and the headless [`Harness`](crate::Harness) identically.
pub const DEFAULT_COMMAND_BUDGET: usize = 1024;

/// Whether the app should keep running after a command settled.
///
/// Crate-internal: the outcome both drivers map onto their own running flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Settled {
    /// No [`Cmd::quit`](crate::Cmd::quit) was reached; keep looping.
    Running,
    /// A [`Cmd::quit`](crate::Cmd::quit) settled; stop the program.
    Quit,
}

/// Folds `cmd` and every message it cascades into [`App::update`],
/// breadth-first and in order, until the work settles or `budget` steps elapse.
///
/// This is the one place command effects are processed, shared verbatim by the
/// live [`run`] loop and the headless [`Harness`](crate::Harness) so their
/// semantics cannot drift. A `quit` consumes the rest of the queue and stops
/// immediately (it is not drained first).
///
/// # Panics
///
/// Panics if more than `budget` steps elapse without settling. An unbounded
/// `update` → `perform` → `update` cycle is unambiguously a reducer bug (not an
/// IO/environment failure), so failing fast with a clear message is correct;
/// the backend guard's `Drop` still restores the terminal while unwinding.
pub(crate) fn settle<A: App>(app: &mut A, cmd: Cmd<A::Message>, budget: usize) -> Settled {
    // Breadth-first: a command's messages are folded in order, and each
    // resulting command is appended, so ordering is deterministic.
    let mut pending = VecDeque::new();
    let mut quit = false;
    cmd.drain(|m| pending.push_back(m), || quit = true);
    if quit {
        return Settled::Quit;
    }

    let mut steps = 0usize;
    while let Some(message) = pending.pop_front() {
        steps += 1;
        assert!(
            steps <= budget,
            "rstui-runtime: command loop exceeded {budget} steps without \
             settling; an update/perform cycle is producing messages without end"
        );
        let next = app.update(message);
        next.drain(|m| pending.push_back(m), || quit = true);
        if quit {
            return Settled::Quit;
        }
    }
    Settled::Running
}

/// Why [`run`] stopped with an error.
///
/// The loop straddles two fallible seams — rendering through a [`Backend`] and
/// reading from an [`EventSource`] — so its error names which one failed. It
/// implements [`std::error::Error`] (with [`source`](std::error::Error::source)
/// pointing at the inner cause) so an application can `?`-bubble it into
/// `Box<dyn Error>` / `anyhow` at `main`.
#[derive(Debug)]
pub enum RunError<R, I> {
    /// A render / terminal-control operation on the [`Backend`] failed.
    Backend(R),
    /// Reading the next input [`Event`](rstui_core::Event) from the
    /// [`EventSource`] failed.
    Input(I),
}

impl<R: fmt::Display, I: fmt::Display> fmt::Display for RunError<R, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(e) => write!(f, "rstui-runtime: render backend error: {e}"),
            Self::Input(e) => write!(f, "rstui-runtime: input source error: {e}"),
        }
    }
}

impl<R, I> std::error::Error for RunError<R, I>
where
    R: std::error::Error + 'static,
    I: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(e) => Some(e),
            Self::Input(e) => Some(e),
        }
    }
}

/// Renders the current app state into a fresh frame and presents it.
///
/// `app` and `terminal` are distinct values, so the view's shared borrow of
/// `app` and `Terminal::draw`'s mutable borrow of `terminal` never conflict
/// (the analogous split inside [`Harness`](crate::Harness) needs a manual
/// reborrow only because both are fields of one struct).
fn render<A: App, B: Backend>(terminal: &mut Terminal<B>, app: &A) -> Result<(), B::Error> {
    terminal.draw(|frame| app.view(frame))?;
    Ok(())
}

/// Runs `app` to completion on a real terminal, returning the final app state.
///
/// Generic over any [`Backend`] and [`EventSource`], so the same call drives a
/// live crossterm terminal or a deterministic
/// [`TestBackend`](rstui_core::TestBackend) +
/// [`TestEventSource`](rstui_core::TestEventSource) in tests. See the
/// [module docs](self) for the loop's exact ordering and its deliberately
/// deferred pieces (threaded commands, ticks, panic hook).
///
/// The loop stops when an [`update`](App::update) returns
/// [`Cmd::quit`](crate::Cmd::quit), or when input is permanently exhausted
/// (`poll_event(None)` yields `Ok(None)` — terminal closed / source drained).
/// On exit the [`Terminal`] (and the backend it owns) is dropped, so a
/// restoring backend guard runs *before* this returns.
///
/// `init`'s command is settled before the first frame, exactly as in
/// [`Harness::new`](crate::Harness::new); an `init` that quits returns without
/// ever polling for input.
///
/// # Errors
///
/// Returns [`RunError::Backend`] if a render / terminal operation fails, or
/// [`RunError::Input`] if reading the next event fails. A runaway reducer
/// (a command cascade that never settles) panics rather than erroring — see
/// [`settle`].
pub fn run<A, B, S>(
    mut app: A,
    backend: B,
    events: &mut S,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    B: Backend,
    S: EventSource,
{
    let mut terminal = Terminal::new(backend).map_err(RunError::Backend)?;

    // Mirror `Harness::new`: run `init`, settle its command, render the first
    // frame — so the screen is meaningful before the first input and an
    // `init` that quits is observed without ever polling.
    let init = app.init();
    let mut running = settle(&mut app, init, DEFAULT_COMMAND_BUDGET) == Settled::Running;
    render(&mut terminal, &app).map_err(RunError::Backend)?;

    while running {
        match events.poll_event(None).map_err(RunError::Input)? {
            Some(event) => {
                // `on_event` (&self) decides intent; `update` (&mut self) is
                // the sole mutation; `settle` folds any follow-up commands.
                // Borrows are strictly sequential, so no reborrow dance.
                if let Some(message) = app.on_event(event) {
                    let cmd = app.update(message);
                    if settle(&mut app, cmd, DEFAULT_COMMAND_BUDGET) == Settled::Quit {
                        running = false;
                    }
                }
                // Repaint even when no message was produced, so a resize the
                // backend already absorbed still repaints (the diff is empty
                // and sends zero cells when nothing actually changed).
                render(&mut terminal, &app).map_err(RunError::Backend)?;
            }
            // Unbounded wait returning `None` => input ended for good: stop.
            None => running = false,
        }
    }

    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cmd;
    use rstui_core::{Cell, Event, KeyCode, KeyEvent, Position, Size, Style, TestBackend};
    use rstui_core::{EventSource, TestEventSource};
    use std::cell::RefCell;
    use std::convert::Infallible;
    use std::rc::Rc;
    use std::time::Duration;

    /// A counter mirroring the harness fixture: `+` increments, `d` increments
    /// then defers another increment via a command, `q` quits.
    #[derive(Default)]
    struct Counter {
        value: i64,
    }

    enum Msg {
        Inc,
        IncThenDefer,
        Quit,
    }

    impl App for Counter {
        type Message = Msg;

        fn on_event(&self, event: Event) -> Option<Msg> {
            match event.as_key_press()?.code {
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

    fn key(c: char) -> Event {
        Event::from(KeyEvent::char(c))
    }

    #[test]
    fn runs_to_quit_and_returns_the_final_app() {
        let mut input = TestEventSource::with_events([key('+'), key('+'), key('q')]);
        let app = run(Counter::default(), TestBackend::new(8, 1), &mut input).unwrap();
        assert_eq!(app.value, 2);
        // `q` was consumed, then the loop stopped — it never polled again.
        assert!(input.is_empty());
    }

    #[test]
    fn exits_cleanly_when_input_is_exhausted() {
        // No `q`: the loop drains the script, then `poll_event(None)` yields
        // `Ok(None)` (end-of-input) and the loop stops on its own.
        let mut input = TestEventSource::with_events([key('+'), key('z'), key('+')]);
        let app = run(Counter::default(), TestBackend::new(8, 1), &mut input).unwrap();
        // The unmapped `z` produced no message and left state untouched.
        assert_eq!(app.value, 2);
    }

    #[test]
    fn init_quit_returns_without_ever_polling() {
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

        // The source is full; an `init` that quits must not consume any of it.
        let mut input = TestEventSource::with_events([key('+'), key('+')]);
        run(Boot, TestBackend::new(4, 1), &mut input).unwrap();
        assert_eq!(input.len(), 2);
    }

    #[test]
    fn live_loop_settles_command_cascades_like_the_harness() {
        // `d` bumps once and a `Cmd::perform` feeds another `Inc` back in;
        // proves the live loop shares the harness's settling core.
        let mut input = TestEventSource::with_events([key('d'), key('q')]);
        let app = run(Counter::default(), TestBackend::new(8, 1), &mut input).unwrap();
        assert_eq!(app.value, 2);
    }

    /// A `Backend` sharing its in-memory surface through an `Rc<RefCell<_>>`
    /// so a test can assert the final frame *after* `run` has consumed and
    /// dropped the terminal — the same shared-handle technique the crossterm
    /// lifecycle guard's panic test uses.
    #[derive(Clone)]
    struct SharedTestBackend(Rc<RefCell<TestBackend>>);

    impl Backend for SharedTestBackend {
        type Error = Infallible;

        fn draw<'a, Iter>(&mut self, cells: Iter) -> Result<(), Self::Error>
        where
            Iter: IntoIterator<Item = (Position, &'a Cell)>,
        {
            self.0.borrow_mut().draw(cells)
        }

        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().hide_cursor()
        }

        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().show_cursor()
        }

        fn cursor_position(&mut self) -> Result<Position, Self::Error> {
            self.0.borrow_mut().cursor_position()
        }

        fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
            self.0.borrow_mut().set_cursor_position(position)
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().clear()
        }

        fn size(&self) -> Result<Size, Self::Error> {
            self.0.borrow().size()
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().flush()
        }
    }

    #[test]
    fn renders_each_frame_through_the_real_loop() {
        let shared = Rc::new(RefCell::new(TestBackend::new(8, 1)));
        let mut input = TestEventSource::with_events([key('+'), key('q')]);

        let app = run(
            Counter::default(),
            SharedTestBackend(Rc::clone(&shared)),
            &mut input,
        )
        .unwrap();

        assert_eq!(app.value, 1);
        // The final frame the production loop presented, asserted end-to-end.
        assert_eq!(format!("{}", shared.borrow()), "n=1     \n");
    }

    #[test]
    #[should_panic(expected = "command loop exceeded")]
    fn unbounded_command_cycle_panics_in_the_live_loop_too() {
        struct Runaway;
        impl App for Runaway {
            type Message = ();
            fn on_event(&self, _: Event) -> Option<()> {
                Some(())
            }
            fn update(&mut self, (): ()) -> Cmd<()> {
                Cmd::perform(|| ())
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }

        let mut input = TestEventSource::with_events([key('x')]);
        // Shares `settle`, so the same budget guard fires in the live loop.
        let _ = run(Runaway, TestBackend::new(1, 1), &mut input);
    }

    // A minimal `std::error::Error` for the error-propagation tests.
    #[derive(Debug)]
    struct Boom;

    impl fmt::Display for Boom {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("boom")
        }
    }

    impl std::error::Error for Boom {}

    /// A backend whose size query succeeds (so `Terminal::new` works) but
    /// whose first `draw` fails — the first render then errors.
    struct FailingBackend;

    impl Backend for FailingBackend {
        type Error = Boom;

        fn draw<'a, Iter>(&mut self, _cells: Iter) -> Result<(), Self::Error>
        where
            Iter: IntoIterator<Item = (Position, &'a Cell)>,
        {
            Err(Boom)
        }

        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn cursor_position(&mut self) -> Result<Position, Self::Error> {
            Ok(Position::ORIGIN)
        }

        fn set_cursor_position(&mut self, _: Position) -> Result<(), Self::Error> {
            Ok(())
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn size(&self) -> Result<Size, Self::Error> {
            Ok(Size::new(4, 1))
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn backend_failure_is_reported_as_run_error_backend() {
        #[derive(Debug)]
        struct Noop;
        impl App for Noop {
            type Message = ();
            fn update(&mut self, (): ()) -> Cmd<()> {
                Cmd::none()
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }

        let mut input = TestEventSource::new();
        let err = run(Noop, FailingBackend, &mut input).unwrap_err();

        assert!(matches!(err, RunError::Backend(Boom)));
        // The `Display`/`Error` impls expose the inner cause for `?`-bubbling.
        assert!(err.to_string().contains("boom"));
        assert!(std::error::Error::source(&err).is_some());
    }

    /// An `EventSource` whose poll always fails, to exercise the input arm.
    struct FailingSource;

    impl EventSource for FailingSource {
        type Error = Boom;

        fn poll_event(&mut self, _: Option<Duration>) -> Result<Option<Event>, Self::Error> {
            Err(Boom)
        }
    }

    #[test]
    fn input_failure_is_reported_as_run_error_input() {
        #[derive(Debug)]
        struct Noop;
        impl App for Noop {
            type Message = ();
            fn update(&mut self, (): ()) -> Cmd<()> {
                Cmd::none()
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }

        // First render succeeds (TestBackend), then the first poll fails.
        let err = run(Noop, TestBackend::new(4, 1), &mut FailingSource).unwrap_err();
        assert!(matches!(err, RunError::Input(Boom)));
        assert!(err.to_string().contains("input source error"));
    }
}
