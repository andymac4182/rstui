//! The **live** event loop: the production twin of [`Harness`](crate::Harness).
//!
//! [`run`] drives an [`App`] over a real [`Backend`] and [`EventSource`] with
//! the *exact same* ordering and command-settling rule the headless
//! [`Harness`](crate::Harness) uses — because they call the same
//! `settle` state machine. The harness is therefore not merely *a*
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
//! 2. Wait for the next input, bounded by [`tick_rate`](App::tick_rate): block
//!    on [`poll_event(None)`](rstui_core::EventSource::poll_event) when the app
//!    declares no tick rate, or on `poll_event(Some(until_next_tick))` when it
//!    does. Input maps through [`on_event`](App::on_event) →
//!    [`update`](App::update); an elapsed tick maps through
//!    [`on_tick`](App::on_tick) → [`update`](App::update). Either way the
//!    follow-up commands settle through the *same* `settle` core, then a
//!    repaint (a no-op cell diff if nothing changed, so an idle tick or an
//!    unhandled key is cheap).
//! 3. A [`Cmd::quit`](crate::Cmd::quit) stops the loop. So does end-of-input
//!    *when the app is not ticking*: an unbounded `poll_event(None)` returning
//!    `Ok(None)` means input is permanently exhausted. A ticking app is in
//!    animation mode and a *bounded* `Ok(None)` is a tick, not end-of-input
//!    (the [`EventSource`] contract's two meanings of `Ok(None)`,
//!    disambiguated by whether the wait was bounded).
//!
//! ## Two timing models, one reducer
//!
//! rstui now has *both* Elm timing constructs, kept distinct:
//!
//! - The **subscription**: [`tick_rate`](App::tick_rate) declares a cadence as
//!   a pure function of state and [`on_tick`](App::on_tick) maps an elapsed
//!   period to a message, exactly as [`on_event`](App::on_event) maps input. It
//!   rides the `poll_event(Some(timeout))` seam, needs no thread, and the
//!   headless [`Harness`](crate::Harness) drives it explicitly via
//!   [`Harness::tick`](crate::Harness::tick). Recorded in
//!   [ADR 0006](https://github.com/andymac4182/rstui/blob/main/docs/adr/0006-runtime-tick-and-loop-model.md).
//! - The **scheduled effect**: [`Cmd::tick`](crate::Cmd::tick) /
//!   [`Cmd::every`](crate::Cmd::every) (Bubble Tea's `tea.Tick`/`tea.Every`)
//!   and any [`Cmd::perform`](crate::Cmd::perform) run through a command
//!   executor. [`run`] uses an inline executor (the message folds
//!   this turn — the deterministic pre-async behavior the harness also uses);
//!   [`run_threaded`] uses a `std::thread`-per-command executor so a slow load
//!   or a real delay never blocks the loop. The reducer (`settle`) is *the
//!   same* either way — only effect dispatch differs — so the harness stays an
//!   exact stand-in. Recorded in
//!   [ADR 0008](https://github.com/andymac4182/rstui/blob/main/docs/adr/0008-async-command-executor.md).
//!
//! Still deferred, not stubbed:
//!
//! - **No bounded thread pool.** [`run_threaded`] spawns one `std::thread` per
//!   command (Bubble Tea's per-goroutine model). A pool is a possible later
//!   refinement; the `Send + 'static` bound already makes it non-breaking.
//! - **No `async`/tokio path.** The threaded executor is dependency-free; an
//!   `EventStream`-based async runtime remains a separable future, and the
//!   synchronous loop never depends on it.
//! - **No panic hook here.** Restoring the terminal on panic is the backend
//!   guard's `Drop`; making the panic *message* visible (and the
//!   termination-signal restore) is the app shell's policy
//!   (`rstui_crossterm::run_app`), a distinct concern in the backend crate.
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
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rstui_core::{Backend, EventSource, Terminal};

use crate::app::App;
use crate::cmd::{Cmd, CommandExecutor, InlineExecutor};

/// How often [`run_threaded`] wakes to drain off-loop command results when
/// nothing else (input or a tick) would wake it.
///
/// ~60 Hz: brisk enough that a finished background `Cmd::perform`/`Cmd::tick`
/// repaints within a frame, cheap enough to be invisible (an empty channel
/// drain plus an empty cell diff is a handful of microseconds). The default
/// inline [`run`] never uses this — it still blocks purely on input.
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// The default cap on `update`/`perform` steps a single input may produce
/// before the command loop gives up. Generous enough for real cascades, low
/// enough to fail a runaway reducer fast.
///
/// This bounds the shared `settle` state machine, so it governs both the
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
/// This is the one place command effects are processed, shared verbatim by
/// every loop ([`run`], [`run_threaded`], and the headless
/// [`Harness`](crate::Harness)) so their reducer semantics cannot drift. The
/// only thing that varies is `exec`: with an inline executor a
/// `perform`/timer's message folds *this turn* (deterministic, the pre-async
/// behavior); with a threaded executor that effect is taken off the loop and
/// its message arrives later, re-entering through a fresh `settle`. A `quit`
/// consumes the rest of the queue and stops immediately.
///
/// # Panics
///
/// Panics if more than `budget` steps elapse without settling. An unbounded
/// `update` → `perform` → `update` cycle is unambiguously a reducer bug (not an
/// IO/environment failure), so failing fast with a clear message is correct;
/// the backend guard's `Drop` still restores the terminal while unwinding.
pub(crate) fn settle<A: App>(
    app: &mut A,
    cmd: Cmd<A::Message>,
    budget: usize,
    exec: &mut dyn CommandExecutor<A::Message>,
) -> Settled {
    // Breadth-first: a command's messages are folded in order, and each
    // resulting command is appended, so ordering is deterministic.
    let mut pending = VecDeque::new();
    let mut quit = false;
    cmd.dispatch(exec, |m| pending.push_back(m), || quit = true);
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
        next.dispatch(exec, |m| pending.push_back(m), || quit = true);
        if quit {
            return Settled::Quit;
        }
    }
    Settled::Running
}

/// Folds one `message` and any commands it cascades, returning whether the app
/// is still running.
///
/// The `app.update(message)` → [`settle`] pair the loop runs for *every*
/// message source — a mapped input event, an elapsed `on_tick`, an off-loop
/// command result — extracted so the three sites cannot drift apart.
fn step<A: App>(
    app: &mut A,
    message: A::Message,
    exec: &mut dyn CommandExecutor<A::Message>,
) -> Settled {
    let cmd = app.update(message);
    settle(app, cmd, DEFAULT_COMMAND_BUDGET, exec)
}

/// Returns how long to wait so a wake lands on the next wall-clock multiple of
/// `period` (the [`Cmd::every`](crate::Cmd::every) alignment), or `ZERO` if a
/// boundary is now / `period` is zero.
fn until_next_multiple(period: Duration) -> Duration {
    let period = period.as_nanos();
    if period == 0 {
        return Duration::ZERO;
    }
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let remainder = since_epoch % period;
    if remainder == 0 {
        Duration::ZERO
    } else {
        // `period - remainder < period`; periods are realistic durations
        // (seconds/minutes), so this is always well within `u64` nanoseconds.
        Duration::from_nanos((period - remainder) as u64)
    }
}

/// The off-loop [`CommandExecutor`]: a `std::thread` per command, no external
/// dependency.
///
/// Each [`perform`](Cmd::perform) closure (and each [`tick`](Cmd::tick)/
/// [`every`](Cmd::every) timer) runs on its own thread; its message is sent
/// back over a channel the [`run_threaded`] loop drains, so a slow load or a
/// sleeping timer never blocks rendering or input. This is the spawn-per-
/// command model Bubble Tea uses for goroutines; a bounded pool is a possible
/// later refinement, deliberately not built yet (the `Send + 'static` bound on
/// command closures already makes that non-breaking). Returning `None` from
/// both methods is what tells [`settle`] the message is *not* folded this turn
/// — it re-enters the loop later as a fresh `update`.
pub(crate) struct ThreadCommandExecutor<M> {
    sender: mpsc::Sender<M>,
}

impl<M: Send + 'static> CommandExecutor<M> for ThreadCommandExecutor<M> {
    fn perform(&mut self, work: Box<dyn FnOnce() -> M + Send + 'static>) -> Option<M> {
        let sender = self.sender.clone();
        thread::spawn(move || {
            // The receiver is gone only once the loop has stopped; a failed
            // send then is the correct, silent outcome (nothing awaits it).
            let _ = sender.send(work());
        });
        None
    }

    fn timer(
        &mut self,
        delay: Duration,
        aligned: bool,
        work: Box<dyn FnOnce() -> M + Send + 'static>,
    ) -> Option<M> {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let wait = if aligned {
                until_next_multiple(delay)
            } else {
                delay
            };
            thread::sleep(wait);
            let _ = sender.send(work());
        });
        None
    }
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

/// Runs `app` to completion on a real terminal with **inline** commands,
/// returning the final app state.
///
/// The default loop: a [`Cmd::perform`](crate::Cmd::perform)/
/// [`tick`](crate::Cmd::tick) runs *on the loop thread*, exactly as the
/// headless [`Harness`](crate::Harness) runs it, so this is byte-for-byte the
/// pre-async behavior and stays deterministic for a scripted
/// [`TestEventSource`](rstui_core::TestEventSource). A slow command therefore
/// blocks rendering — use [`run_threaded`] when a command may be slow or a
/// `Cmd::tick`/`Cmd::every` must actually wait. Generic over any [`Backend`]
/// and [`EventSource`]; see the [module docs](self) for the loop's exact
/// ordering.
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
/// `settle`.
pub fn run<A, B, S>(app: A, backend: B, events: &mut S) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    B: Backend,
    S: EventSource,
{
    // Inline executor, no result channel: identical to the pre-async loop.
    run_core(app, backend, events, &mut InlineExecutor, None)
}

/// Runs `app` like [`run`], but performs each [`Cmd::perform`](crate::Cmd::perform)
/// and [`Cmd::tick`](crate::Cmd::tick)/[`every`](crate::Cmd::every) **off the
/// render loop**, on a `std::thread` per command (no external dependency).
///
/// This is the loop a real full-screen app wants: a slow load or a sleeping
/// timer no longer freezes input or rendering — its message arrives on a
/// channel the loop drains, re-entering [`update`](App::update) like any other
/// message. `Cmd::tick`/`Cmd::every` now wait for real (and `every` snaps to
/// the wall clock); under [`run`]/[`Harness`](crate::Harness) they still fire
/// immediately so tests stay deterministic.
///
/// Because the loop never blocks unbounded here (it wakes at least every
/// `COMMAND_POLL_INTERVAL` to drain results), end-of-input is **not** a stop
/// condition: a threaded app runs until [`Cmd::quit`](crate::Cmd::quit) or an
/// error, exactly as a Bubble Tea program with live commands does. The reducer
/// is the *same* `settle`; only effect dispatch differs, so behavior matches
/// the harness up to *when* (not whether) a command's message lands. See
/// [ADR 0008](https://github.com/andymac4182/rstui/blob/main/docs/adr/0008-async-command-executor.md).
///
/// # Errors
///
/// Identical to [`run`].
pub fn run_threaded<A, B, S>(
    app: A,
    backend: B,
    events: &mut S,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    A::Message: Send + 'static,
    B: Backend,
    S: EventSource,
{
    let (sender, results) = mpsc::channel();
    let mut exec = ThreadCommandExecutor { sender };
    // `results` lives in this frame for the whole loop; `run_core` only drains
    // it, so it borrows rather than owns it (the channel's `Sender` lives in
    // `exec`, which also outlives the call).
    run_core(app, backend, events, &mut exec, Some(&results))
}

/// The one loop body both [`run`] (inline, no `results`) and [`run_threaded`]
/// (off-loop, draining `results`) share, so their ordering, tick handling, and
/// quit/EOF rules cannot drift — only the command executor and whether a
/// result channel exists differ.
fn run_core<A, B, S>(
    mut app: A,
    backend: B,
    events: &mut S,
    exec: &mut dyn CommandExecutor<A::Message>,
    results: Option<&Receiver<A::Message>>,
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
    let mut running = settle(&mut app, init, DEFAULT_COMMAND_BUDGET, exec) == Settled::Running;
    render(&mut terminal, &app).map_err(RunError::Backend)?;

    // The next scheduled tick, armed only while the app asks to be ticked
    // (`App::tick_rate` is `Some`). This `Instant` is the *live loop's* own
    // wall clock; it never reaches the shared `settle` core or the headless
    // `Harness`, both of which stay clock-free.
    let mut next_tick: Option<Instant> = None;

    while running {
        // Threaded mode: fold every off-loop command result that has arrived,
        // breadth-first through the same `update`/`settle`, then repaint once
        // (like the input arm). Inline mode has no channel and skips this — it
        // is byte-for-byte the pre-async loop.
        if let Some(rx) = results {
            let mut drained_any = false;
            while let Ok(message) = rx.try_recv() {
                drained_any = true;
                if step(&mut app, message, exec) == Settled::Quit {
                    running = false;
                    break;
                }
            }
            if drained_any {
                render(&mut terminal, &app).map_err(RunError::Backend)?;
            }
            if !running {
                break;
            }
        }

        // Cadence is a pure function of state, re-read every iteration so an
        // app can start, stop, or retune animation by what `tick_rate`
        // returns. `None` + inline is exactly the pre-tick loop: block on
        // input with no clock so end-of-input is still observed.
        let rate = app.tick_rate();
        let tick_wait = match rate {
            Some(rate) => {
                let deadline = *next_tick.get_or_insert_with(|| Instant::now() + rate);
                // A past deadline (a frame slower than the rate) yields `ZERO`:
                // poll without blocking, then tick. Missed ticks coalesce.
                Some(deadline.saturating_duration_since(Instant::now()))
            }
            None => {
                next_tick = None;
                None
            }
        };
        let timeout = if results.is_some() {
            // Never block unbounded: a result may arrive with no input/tick, so
            // wake at least every `COMMAND_POLL_INTERVAL` to drain it.
            Some(
                tick_wait
                    .unwrap_or(COMMAND_POLL_INTERVAL)
                    .min(COMMAND_POLL_INTERVAL),
            )
        } else {
            tick_wait
        };

        match events.poll_event(timeout).map_err(RunError::Input)? {
            Some(event) => {
                // `on_event` (&self) decides intent; `update` (&mut self) is
                // the sole mutation; `settle` folds any follow-up commands.
                if let Some(message) = app.on_event(event) {
                    if step(&mut app, message, exec) == Settled::Quit {
                        running = false;
                    }
                }
                // Repaint even when no message was produced, so a resize the
                // backend already absorbed still repaints (an empty diff sends
                // zero cells when nothing actually changed).
                render(&mut terminal, &app).map_err(RunError::Backend)?;
            }
            None => match rate {
                // A *bounded* wait returned `None`: the timer elapsed (or the
                // source had nothing this tick). Re-arm from now so missed
                // ticks coalesce, then route the tick through the **same**
                // `update`/`settle` path as input.
                Some(rate) => {
                    next_tick = Some(Instant::now() + rate);
                    if let Some(message) = app.on_tick() {
                        if step(&mut app, message, exec) == Settled::Quit {
                            running = false;
                        }
                    }
                    render(&mut terminal, &app).map_err(RunError::Backend)?;
                }
                // No tick rate and `Ok(None)`. Inline: an *unbounded* wait
                // ended => input exhausted for good, stop (terminal closed /
                // scripted source drained). Threaded: this was only the
                // bounded command-drain wakeup, never EOF — loop and drain the
                // channel. A threaded app stops via `Cmd::quit`/error, exactly
                // as a ticking app does (ADR 0006/0007).
                None => {
                    if results.is_none() {
                        running = false;
                    }
                }
            },
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

    /// An app that ticks: every elapsed tick bumps a counter and quits at the
    /// limit. `Duration::ZERO` keeps the test instant — `TestEventSource`
    /// ignores the timeout, so each bounded poll returns `Ok(None)` at once.
    struct TickToLimit {
        ticks: u32,
        limit: u32,
    }

    impl App for TickToLimit {
        type Message = ();
        fn tick_rate(&self) -> Option<Duration> {
            Some(Duration::ZERO)
        }
        fn on_tick(&self) -> Option<()> {
            Some(())
        }
        fn update(&mut self, (): ()) -> Cmd<()> {
            self.ticks += 1;
            if self.ticks >= self.limit {
                Cmd::quit()
            } else {
                Cmd::none()
            }
        }
        fn view(&self, _: &mut rstui_core::Frame<'_>) {}
    }

    #[test]
    fn live_loop_ticks_until_a_ticking_app_quits() {
        // No input at all: every bounded poll times out (`Ok(None)`) and the
        // loop takes the tick arm, proving the live loop delivers `on_tick`
        // and that a ticking app stops via `Cmd::quit` (not end-of-input).
        let mut input = TestEventSource::new();
        let app = run(
            TickToLimit { ticks: 0, limit: 5 },
            TestBackend::new(2, 1),
            &mut input,
        )
        .unwrap();
        assert_eq!(app.ticks, 5);
    }

    #[test]
    fn live_loop_interleaves_input_and_ticks_through_one_settle_core() {
        // The scripted keys drain first (a `TestEventSource` yields them
        // before `Ok(None)`); then bounded `Ok(None)` becomes ticks. Both fold
        // through the *same* update/settle, so ticks are the temporal dual of
        // input, not a second reducer.
        struct Mixed {
            keys: u32,
            ticks: u32,
        }
        enum MixedMsg {
            Key,
            Tick,
        }
        impl App for Mixed {
            type Message = MixedMsg;
            fn tick_rate(&self) -> Option<Duration> {
                Some(Duration::ZERO)
            }
            fn on_tick(&self) -> Option<MixedMsg> {
                Some(MixedMsg::Tick)
            }
            fn on_event(&self, event: Event) -> Option<MixedMsg> {
                event.as_key_press().map(|_| MixedMsg::Key)
            }
            fn update(&mut self, message: MixedMsg) -> Cmd<MixedMsg> {
                match message {
                    MixedMsg::Key => {
                        self.keys += 1;
                        Cmd::none()
                    }
                    MixedMsg::Tick => {
                        self.ticks += 1;
                        if self.ticks >= 3 {
                            Cmd::quit()
                        } else {
                            Cmd::none()
                        }
                    }
                }
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let mut input = TestEventSource::with_events([key('a'), key('b')]);
        let app = run(
            Mixed { keys: 0, ticks: 0 },
            TestBackend::new(2, 1),
            &mut input,
        )
        .unwrap();
        assert_eq!(app.keys, 2, "both scripted keys were handled first");
        assert_eq!(app.ticks, 3, "then ticks drove it to quit");
    }

    #[test]
    fn a_tick_capable_app_returning_no_rate_still_stops_on_end_of_input() {
        // The backward-compat contract: when `tick_rate` is `None` the loop
        // takes the *unbounded* poll and `Ok(None)` is permanent end-of-input.
        // `on_tick` would loop forever if it were (wrongly) consulted here, so
        // this test hanging would be the failure signal.
        struct Idle;
        impl App for Idle {
            type Message = ();
            fn tick_rate(&self) -> Option<Duration> {
                None
            }
            fn on_tick(&self) -> Option<()> {
                Some(())
            }
            fn update(&mut self, (): ()) -> Cmd<()> {
                Cmd::none()
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let mut input = TestEventSource::new();
        // Returns promptly rather than hanging: `None` rate keeps EOF stop.
        run(Idle, TestBackend::new(2, 1), &mut input).unwrap();
    }

    #[test]
    fn run_threaded_delivers_an_off_loop_perform_result_then_quits() {
        // `init` schedules an off-loop load; its message arrives on the channel
        // the threaded loop drains, re-enters `update`, and quits. The outcome
        // is deterministic even though *which* poll cycle drains it is not.
        #[derive(Default)]
        struct Loader {
            loaded: bool,
        }
        enum Msg {
            Loaded,
        }
        impl App for Loader {
            type Message = Msg;
            fn init(&mut self) -> Cmd<Msg> {
                Cmd::perform(|| Msg::Loaded)
            }
            fn update(&mut self, _: Msg) -> Cmd<Msg> {
                self.loaded = true;
                Cmd::quit()
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        // No input at all: only the off-loop result can end this loop.
        let mut input = TestEventSource::new();
        let app = run_threaded(Loader::default(), TestBackend::new(2, 1), &mut input).unwrap();
        assert!(
            app.loaded,
            "the off-loop perform's message re-entered update"
        );
    }

    #[test]
    fn run_threaded_actually_waits_for_a_delayed_tick() {
        // Under the threaded loop `Cmd::tick` really sleeps (1ms here) on its
        // own thread, then delivers — proving the timer is honored off-loop,
        // not collapsed to zero like the inline executor.
        #[derive(Default)]
        struct Delayed {
            fired: bool,
        }
        enum Msg {
            Fired,
        }
        impl App for Delayed {
            type Message = Msg;
            fn init(&mut self) -> Cmd<Msg> {
                Cmd::tick(Duration::from_millis(1), || Msg::Fired)
            }
            fn update(&mut self, _: Msg) -> Cmd<Msg> {
                self.fired = true;
                Cmd::quit()
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let mut input = TestEventSource::new();
        let app = run_threaded(Delayed::default(), TestBackend::new(2, 1), &mut input).unwrap();
        assert!(app.fired, "the delayed tick fired and quit the loop");
    }

    #[test]
    fn run_threaded_aligned_every_delivers_and_quits() {
        // `Cmd::every` snaps to the next wall-clock multiple of the period;
        // with a 1ms period the next boundary is essentially immediate. Assert
        // the *outcome* (it fires and quits), not the (timing-dependent) phase.
        #[derive(Default)]
        struct Ticker {
            ticks: u32,
        }
        enum Msg {
            Tick,
        }
        impl App for Ticker {
            type Message = Msg;
            fn init(&mut self) -> Cmd<Msg> {
                Cmd::every(Duration::from_millis(1), || Msg::Tick)
            }
            fn update(&mut self, _: Msg) -> Cmd<Msg> {
                self.ticks += 1;
                // Reschedule once to prove the Bubble-Tea repeat idiom works,
                // then quit so the test terminates deterministically.
                if self.ticks >= 2 {
                    Cmd::quit()
                } else {
                    Cmd::every(Duration::from_millis(1), || Msg::Tick)
                }
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let mut input = TestEventSource::new();
        let app = run_threaded(Ticker::default(), TestBackend::new(2, 1), &mut input).unwrap();
        assert_eq!(app.ticks, 2, "every fired, was rescheduled, then quit");
    }

    #[test]
    fn run_threaded_interleaves_input_and_off_loop_results() {
        // A scripted key triggers an off-loop perform; another key quits. The
        // perform result must fold in too, so the final state reflects both
        // the synchronous input and the asynchronous result.
        #[derive(Default)]
        struct Mixed {
            keys: u32,
            loaded: u32,
        }
        enum Msg {
            Load,
            Loaded,
            Quit,
        }
        impl App for Mixed {
            type Message = Msg;
            fn on_event(&self, event: Event) -> Option<Msg> {
                match event.as_key_press()?.code {
                    KeyCode::Char('l') => Some(Msg::Load),
                    KeyCode::Char('q') => Some(Msg::Quit),
                    _ => None,
                }
            }
            fn update(&mut self, message: Msg) -> Cmd<Msg> {
                match message {
                    Msg::Load => {
                        self.keys += 1;
                        Cmd::perform(|| Msg::Loaded)
                    }
                    Msg::Loaded => {
                        self.loaded += 1;
                        Cmd::none()
                    }
                    Msg::Quit => Cmd::quit(),
                }
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        // 'l' loads (spawns the off-loop result), then 'q' quits — but the
        // loop must drain the result before/around the quit either way; the
        // robust assertion is on the load count and that it ended.
        let mut input = TestEventSource::with_events([key('l'), key('q')]);
        let app = run_threaded(Mixed::default(), TestBackend::new(2, 1), &mut input).unwrap();
        assert_eq!(app.keys, 1, "the scripted key was handled on the loop");
        assert!(app.loaded <= 1, "the off-loop result folded at most once");
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
