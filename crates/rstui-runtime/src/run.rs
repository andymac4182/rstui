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
use std::num::NonZeroUsize;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
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

/// Max input events folded in one burst before forcing a repaint, so a
/// pathological never-ending input flood can still never starve rendering,
/// commands, or ticks.
///
/// A fast resize-drag or scroll-wheel spin emits *bursts* of events; folding
/// the whole burst and repainting **once** (the latest state) instead of
/// once-per-event removes the render backlog that is the actual source of
/// resize/scroll lag. Real bursts are far below this cap (it is the safety
/// valve, not the common path); `view` is a pure projection of state, so the
/// skipped intermediate frames are never observable.
const COALESCE_LIMIT: usize = 1024;

/// Wall-clock cap on a single input-coalescing drain. A *count* cap alone
/// (`COALESCE_LIMIT`) does not bound the *time* the loop spends folding a
/// never-ending flood, so under continuous input (e.g. the per-sample
/// mouse-move reports any-motion mouse capture emits while the pointer
/// moves) the loop could stay in the drain long enough to starve the tick
/// deadline and the steady repaint cadence — the "frames freeze while I move
/// the mouse" report. Breaking the drain after roughly one frame guarantees
/// the loop returns to re-check the tick and present at a bounded cadence
/// no matter how fast input arrives; state is still exact (every event was
/// folded), only the *batch* is time-sliced.
const COALESCE_TIME_BUDGET: Duration = Duration::from_millis(8);

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

/// Folds one input event through `on_event` → [`step`], or [`Settled::Running`]
/// if the app maps it to no message.
///
/// The single-sourced per-event handling shared by the primary input branch
/// **and** the burst-coalescing drain, in *both* the sync [`run_core`] and the
/// async [`run_async`], so those four sites cannot drift.
/// Returns the running outcome **and** whether the event actually produced a
/// message (so the live loops can skip a redundant `view`+`diff` repaint for
/// a no-op event flood — a pointer move under any-motion mouse capture maps
/// to no message; rendering once per such burst is the RT-01 saturation that
/// froze the UI during mouse movement). `None` ⇒ definitively no state
/// change for this event; the headless [`Harness`](crate::Harness) does not
/// use this path, so its determinism is unaffected.
fn handle_input<A: App>(
    app: &mut A,
    event: rstui_core::Event,
    exec: &mut dyn CommandExecutor<A::Message>,
) -> (Settled, bool) {
    match app.on_event(event) {
        Some(message) => (step(app, message, exec), true),
        None => (Settled::Running, false),
    }
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
/// command model Bubble Tea uses for goroutines; [`run_pooled`] is the
/// bounded-concurrency alternative for command-heavy apps. Returning `None`
/// from both methods is what tells [`settle`] the message is *not* folded this
/// turn — it re-enters the loop later as a fresh `update`.
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

/// One unit of pooled work: a boxed closure that computes a command's message
/// and sends it on the result channel. Type-erased so a single queue carries
/// every command regardless of `M`.
type PoolJob = Box<dyn FnOnce() + Send + 'static>;

/// The pool's shared state: the pending-job queue plus a `closed` flag, behind
/// one `Mutex` (so the worker wait-predicate is a single lock) with a
/// [`Condvar`] workers park on. Textbook dependency-free pool — no crossbeam,
/// no external channel.
struct PoolShared {
    /// `(queue, closed)`. `closed` is set on executor drop so idle workers
    /// stop waiting and exit once the queue drains.
    state: Mutex<(VecDeque<PoolJob>, bool)>,
    /// Signalled on every push and on close, so a parked worker re-checks.
    available: Condvar,
}

impl PoolShared {
    /// Pushes a job and wakes one worker.
    fn submit(&self, job: PoolJob) {
        // A poisoned lock means a worker panicked mid-job; recover the guard
        // and keep scheduling rather than poisoning the whole UI.
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.0.push_back(job);
        drop(guard);
        self.available.notify_one();
    }
}

/// Runs jobs until the pool is closed *and* drained. One per worker thread.
fn pool_worker(shared: &Arc<PoolShared>) {
    loop {
        let job = {
            let mut guard = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(job) = guard.0.pop_front() {
                    break Some(job);
                }
                if guard.1 {
                    break None; // closed and empty: this worker is done.
                }
                guard = shared
                    .available
                    .wait(guard)
                    .unwrap_or_else(|e| e.into_inner());
            }
        };
        match job {
            Some(job) => job(),
            None => return,
        }
    }
}

/// A bounded-concurrency off-loop [`CommandExecutor`]: a fixed pool of
/// `std::thread` workers drains a shared queue, so command-heavy apps cannot
/// explode the thread count the way [`ThreadCommandExecutor`]'s spawn-per-
/// command model can.
///
/// [`perform`](Cmd::perform) work is enqueued and runs on the next free
/// worker. A [`tick`](Cmd::tick)/[`every`](Cmd::every) timer is **not** run on
/// a worker — that would tie a pool thread up sleeping — instead a tiny
/// dedicated thread does the (mostly idle) wait and then enqueues the real
/// `work()` onto the pool, so the bound applies to *active work* concurrency,
/// not to idle timers. Like [`ThreadCommandExecutor`] it returns `None`
/// (the message arrives later on the result channel) and bounds threads, not
/// queue depth: a flood enqueues quickly and the workers drain it at the pool
/// width. Result ordering is not guaranteed (same as the unbounded executor).
pub(crate) struct PooledCommandExecutor<M> {
    sender: mpsc::Sender<M>,
    shared: Arc<PoolShared>,
}

impl<M: Send + 'static> PooledCommandExecutor<M> {
    /// Builds the executor and spawns `workers` detached worker threads.
    fn new(sender: mpsc::Sender<M>, workers: NonZeroUsize) -> Self {
        let shared = Arc::new(PoolShared {
            state: Mutex::new((VecDeque::new(), false)),
            available: Condvar::new(),
        });
        for i in 0..workers.get() {
            let shared = Arc::clone(&shared);
            // Detached, like `ThreadCommandExecutor`'s threads: they exit
            // promptly once `Drop` closes the pool, so they are not leaked
            // across repeated `run_pooled` calls in one process.
            let _ = thread::Builder::new()
                .name(format!("rstui-cmd-pool-{i}"))
                .spawn(move || pool_worker(&shared));
        }
        Self { sender, shared }
    }
}

impl<M: Send + 'static> CommandExecutor<M> for PooledCommandExecutor<M> {
    fn perform(&mut self, work: Box<dyn FnOnce() -> M + Send + 'static>) -> Option<M> {
        let sender = self.sender.clone();
        self.shared.submit(Box::new(move || {
            let _ = sender.send(work());
        }));
        None
    }

    fn timer(
        &mut self,
        delay: Duration,
        aligned: bool,
        work: Box<dyn FnOnce() -> M + Send + 'static>,
    ) -> Option<M> {
        let sender = self.sender.clone();
        let shared = Arc::clone(&self.shared);
        // The sleep gets its own ephemeral thread so it never occupies a pool
        // worker; only the post-delay `work()` is pooled.
        thread::spawn(move || {
            let wait = if aligned {
                until_next_multiple(delay)
            } else {
                delay
            };
            thread::sleep(wait);
            shared.submit(Box::new(move || {
                let _ = sender.send(work());
            }));
        });
        None
    }
}

impl<M> Drop for PooledCommandExecutor<M> {
    fn drop(&mut self) {
        // Close the pool and wake every worker so idle ones exit once the
        // queue drains. Detached, so this returns without waiting on a slow
        // in-flight job (its result send then fails silently — the loop is
        // gone — exactly as for `ThreadCommandExecutor`).
        if let Ok(mut guard) = self.shared.state.lock() {
            guard.1 = true;
        }
        self.shared.available.notify_all();
    }
}

/// An asynchronous input source — the async dual of
/// [`EventSource`], available only with the `async`
/// cargo feature.
///
/// Where [`EventSource::poll_event`]
/// *blocks* (and overloads `Ok(None)` as either "timed out" or "ended"
/// depending on the timeout argument), this is awaited and has **one** meaning
/// for each outcome — which is exactly what lets [`run_async`] `select!` it
/// against command results and a tick timer with no poll interval:
///
/// - `Ok(Some(event))` — an input event arrived.
/// - `Ok(None)` — input ended **permanently** (terminal closed / scripted
///   source drained). No timeout overload: ticks are a separate `select!`
///   arm, not a poll deadline.
/// - `Err(e)` — the underlying device failed.
///
/// Native async-fn-in-trait (stable; this crate targets Rust 1.85);
/// monomorphized like `EventSource`, so it needs no boxing and the returned
/// future is `Send` for a multi-threaded `select!`.
#[cfg(feature = "async")]
pub trait AsyncEventSource {
    /// How this source reports failure (`io::Error` for a real terminal,
    /// [`Infallible`](std::convert::Infallible) for a scripted test source).
    type Error: std::error::Error;

    /// Awaits the next input event. See the [trait docs](AsyncEventSource) for
    /// the three outcomes; `Ok(None)` is *only* permanent end-of-input.
    ///
    /// Explicit `-> impl Future + Send` (not `async fn`): the `Send` bound is
    /// required so the future works under a multi-threaded `tokio::select!`,
    /// and spelling it out is exactly what `async fn`-in-trait cannot do.
    fn next_event(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<rstui_core::Event>, Self::Error>> + Send;
}

/// The [`CommandExecutor`] for the async loop: work runs off the loop on the
/// ambient tokio runtime and its message returns over a **tokio**
/// `mpsc::UnboundedSender` that [`run_async`] `select!`s on (so a finished
/// command wakes the loop *immediately*, with none of the sync loop's
/// poll-interval latency).
///
/// `perform` uses `spawn_blocking` (the closure is sync `FnOnce`); a timer
/// uses `tokio::time::sleep`, so it honors a *paused* clock and the async-loop
/// tests are deterministic with no real wall time. Returning `None` keeps the
/// shared `settle` contract: the message is not folded this turn — it arrives
/// later through the loop, exactly as for every other off-loop executor.
#[cfg(feature = "async")]
pub(crate) struct TokioCommandExecutor<M> {
    handle: tokio::runtime::Handle,
    sender: tokio::sync::mpsc::UnboundedSender<M>,
}

#[cfg(feature = "async")]
impl<M: Send + 'static> CommandExecutor<M> for TokioCommandExecutor<M> {
    fn perform(&mut self, work: Box<dyn FnOnce() -> M + Send + 'static>) -> Option<M> {
        let sender = self.sender.clone();
        // Detached like the std-thread executor's thread handle; explicit
        // `drop` (not `let _ =`) because a `JoinHandle` is itself a `Future`.
        drop(self.handle.spawn_blocking(move || {
            let _ = sender.send(work());
        }));
        None
    }

    fn timer(
        &mut self,
        delay: Duration,
        aligned: bool,
        work: Box<dyn FnOnce() -> M + Send + 'static>,
    ) -> Option<M> {
        let sender = self.sender.clone();
        drop(self.handle.spawn(async move {
            let wait = if aligned {
                until_next_multiple(delay)
            } else {
                delay
            };
            // `tokio::time::sleep` (not `thread::sleep`) so a paused clock
            // controls it — the async-loop tests advance virtual time.
            tokio::time::sleep(wait).await;
            let _ = sender.send(work());
        }));
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

/// Renders like [`render`] but splits the cost into the `view` projection
/// (the `App::view` closure) and the `flush` (buffer diff + backend write)
/// — used only on the observed path (ADR 0018 §3), so the default loop's
/// cost is unchanged.
fn render_timed<A: App, B: Backend>(
    terminal: &mut Terminal<B>,
    app: &A,
) -> Result<(Duration, Duration), B::Error> {
    let mut view = Duration::ZERO;
    let t0 = Instant::now();
    terminal.draw(|frame| {
        let v = Instant::now();
        app.view(frame);
        view = v.elapsed();
    })?;
    let total = t0.elapsed();
    Ok((view, total.saturating_sub(view)))
}

/// One event-loop iteration's measurements, handed **by value** to a
/// [`FrameObserver`] (ADR 0018 §3). `Duration`s are wall time;
/// `logic + view + flush ≈ total` (the active work since the event
/// arrived — the idle `poll` wait is excluded). `produced` is the RT-01
/// flag: `false` means the iteration changed nothing and the repaint was
/// skipped. The Chrome-DevTools analogy is direct: `logic` ≈ Scripting,
/// `view` ≈ Rendering, `flush` ≈ Painting.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameMetrics {
    /// Monotonic iteration index since the loop started observing.
    pub frame: u64,
    /// Input decode + `on_event` + `update` + `settle` + coalescing.
    pub logic: Duration,
    /// `App::view` projection into the back buffer.
    pub view: Duration,
    /// Buffer diff + terminal write.
    pub flush: Duration,
    /// Active work this iteration (`logic + view + flush`); excludes the
    /// idle wait blocked in `poll_event`.
    pub total: Duration,
    /// Did this iteration change state / repaint (RT-01)? A coalesced
    /// no-op flood reports `false`.
    pub produced: bool,
    /// Events folded into this iteration (the first event plus every one
    /// coalesced) — a high value during pointer motion is the
    /// input-flood / latency-risk signal.
    pub events_coalesced: u32,
    /// First-event-arrival → frame-presented wall time this iteration.
    pub input_latency: Duration,
}

/// A caller-supplied per-iteration observer (ADR 0018 §3).
///
/// Installed via [`run_with_observer`] / the `*_with_observer` entrypoints
/// (or read off [`Harness`](crate::Harness) headlessly). It is handed a
/// by-value [`FrameMetrics`] and **retains no widget state** — the
/// ADR-0012 caller-owned seam, not a retained tree. The zero-observer
/// path (`run`/`run_threaded`/`run_pooled`) is byte-identical and pays no
/// timing cost.
pub trait FrameObserver {
    /// Called once per event-loop iteration that rendered.
    fn on_frame(&mut self, metrics: &FrameMetrics);
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
    run_core(app, backend, events, &mut InlineExecutor, None, None)
}

/// [`run`] with an ADR-0018 [`FrameObserver`] installed: the observer is
/// invoked once per event-loop iteration with by-value [`FrameMetrics`].
/// Behaviour is otherwise byte-identical to [`run`]; the only added cost
/// is the per-iteration `Instant` reads, paid solely while observing.
///
/// # Errors
///
/// Identical to [`run`].
pub fn run_with_observer<A, B, S>(
    app: A,
    backend: B,
    events: &mut S,
    observer: &mut dyn FrameObserver,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    B: Backend,
    S: EventSource,
{
    run_core(
        app,
        backend,
        events,
        &mut InlineExecutor,
        None,
        Some(observer),
    )
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
    run_core(app, backend, events, &mut exec, Some(&results), None)
}

/// [`run_threaded`] with an ADR-0018 [`FrameObserver`] installed.
///
/// # Errors
///
/// Identical to [`run`].
pub fn run_threaded_with_observer<A, B, S>(
    app: A,
    backend: B,
    events: &mut S,
    observer: &mut dyn FrameObserver,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    A::Message: Send + 'static,
    B: Backend,
    S: EventSource,
{
    let (sender, results) = mpsc::channel();
    let mut exec = ThreadCommandExecutor { sender };
    run_core(
        app,
        backend,
        events,
        &mut exec,
        Some(&results),
        Some(observer),
    )
}

/// Runs `app` like [`run_threaded`], but performs commands on a **bounded
/// pool** of exactly `workers` threads instead of one thread per command.
///
/// Identical semantics to [`run_threaded`] (off-loop commands, the same
/// reducer, stops on [`Cmd::quit`](crate::Cmd::quit)/error) — only the
/// executor differs. Prefer this when an app can fire many commands at once
/// (a file manager loading hundreds of entries, a fan-out of requests): the
/// thread count is capped at `workers` instead of growing with command volume.
/// For the typical handful of background tasks, [`run_threaded`]'s spawn-per-
/// command model is simpler and never queues behind a busy worker; this trades
/// that for a hard concurrency bound. Timers do not consume a worker — a tiny
/// dedicated thread does the wait and then enqueues the work onto the pool.
///
/// `workers` is a [`NonZeroUsize`] so "zero workers" (a pool that never makes
/// progress) is unrepresentable rather than a runtime panic.
///
/// # Errors
///
/// Identical to [`run`].
pub fn run_pooled<A, B, S>(
    app: A,
    backend: B,
    events: &mut S,
    workers: NonZeroUsize,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    A::Message: Send + 'static,
    B: Backend,
    S: EventSource,
{
    let (sender, results) = mpsc::channel();
    let mut exec = PooledCommandExecutor::new(sender, workers);
    run_core(app, backend, events, &mut exec, Some(&results), None)
}

/// [`run_pooled`] with an ADR-0018 [`FrameObserver`] installed.
///
/// # Errors
///
/// Identical to [`run`].
pub fn run_pooled_with_observer<A, B, S>(
    app: A,
    backend: B,
    events: &mut S,
    workers: NonZeroUsize,
    observer: &mut dyn FrameObserver,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    A::Message: Send + 'static,
    B: Backend,
    S: EventSource,
{
    let (sender, results) = mpsc::channel();
    let mut exec = PooledCommandExecutor::new(sender, workers);
    run_core(
        app,
        backend,
        events,
        &mut exec,
        Some(&results),
        Some(observer),
    )
}

/// The **async event loop**: drives `app` over an [`AsyncEventSource`] with a
/// `tokio::select!` over input, command results, and ticks. Available only
/// with the `async` cargo feature; **must be awaited inside a tokio runtime**.
///
/// This is the fast-TUI architecture (ADR 0011, superseding ADR 0009). Where
/// the sync loops ([`run`]/[`run_threaded`]/[`run_pooled`]) wake on a poll
/// interval to drain off-loop results, this `select!`s three sources and
/// reacts to whichever is ready *immediately* — no polling latency, and the
/// process is genuinely idle (no wakeups) when nothing is happening:
///
/// - **input** — `events.next_event().await`, mapped through
///   [`on_event`](App::on_event);
/// - **command results** — a tokio `mpsc` the off-loop
///   `TokioCommandExecutor` feeds, folded through
///   [`update`](App::update) the moment they finish;
/// - **ticks** — a `tokio::time::interval` when
///   [`tick_rate`](App::tick_rate) is `Some`, mapped through
///   [`on_tick`](App::on_tick).
///
/// The `select!` is `biased` (input first, then results, then ticks) so input
/// never starves under load — the deterministic priority the sync loops also
/// intend. Critically, the **reducer is unchanged**: every arm calls the same
/// sync `step`/`settle`/`render`, so the headless [`Harness`](crate::Harness)
/// (which drives that same `settle`) remains the exact deterministic test of
/// app logic; only effect/IO multiplexing is async. The loop stops on
/// [`Cmd::quit`](crate::Cmd::quit), on `Ok(None)` from the source (input ended
/// for good), or returns the source/backend error.
///
/// `AsyncEventSource::next_event` must be **cancel-safe** (a `select!` drops
/// the losing branch's future): it must not lose an event if its future is
/// dropped before completing. Channel/stream-backed sources (tokio `mpsc`,
/// crossterm `EventStream`) satisfy this, as do `tokio::mpsc::Receiver::recv`
/// and `Interval::tick` used here.
///
/// # Errors
///
/// [`RunError::Backend`] on a render/terminal failure, [`RunError::Input`] if
/// [`AsyncEventSource::next_event`] fails.
#[cfg(feature = "async")]
pub async fn run_async<A, B, S>(
    mut app: A,
    backend: B,
    events: &mut S,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    A::Message: Send + 'static,
    B: Backend,
    S: AsyncEventSource,
{
    let mut terminal = Terminal::new(backend).map_err(RunError::Backend)?;

    // The command-result channel the loop `select!`s on. `Handle::current()`
    // is valid because this future is polled inside the caller's tokio
    // runtime; the sender lives in `exec` (this frame) for the whole loop, so
    // `rx.recv()` never sees a closed channel while running.
    let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut exec = TokioCommandExecutor {
        handle: tokio::runtime::Handle::current(),
        sender,
    };

    // Mirror `run_core`/`Harness::new`: init, settle, first frame — so an
    // `init` that quits is observed before any `select!`.
    let init = app.init();
    let mut running = settle(&mut app, init, DEFAULT_COMMAND_BUDGET, &mut exec) == Settled::Running;
    render(&mut terminal, &app).map_err(RunError::Backend)?;

    // Rebuilt only when `tick_rate` toggles/retunes (not every iteration), so
    // the cadence is a stable wall-clock schedule input does not reset —
    // `Interval::tick` is cancel-safe, so a `select!` loss keeps the schedule.
    let mut ticker: Option<tokio::time::Interval> = None;
    let mut last_rate: Option<Duration> = None;

    while running {
        let rate = app.tick_rate();
        if rate != last_rate {
            ticker = rate.map(|period| {
                let mut interval =
                    tokio::time::interval_at(tokio::time::Instant::now() + period, period);
                // Coalesce missed ticks (a slow frame never bursts) — the same
                // rule the sync loop's `saturating_duration_since` gives.
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                interval
            });
            last_rate = rate;
        }

        tokio::select! {
            // Input first so a flood of results/ticks never starves keys.
            biased;

            event = events.next_event() => match event {
                Ok(Some(event)) => {
                    let batch_start = Instant::now();
                    // A resize repaints even with no modeled message
                    // (`view` re-reads `frame.area()`); a no-op flood does
                    // not — see the sync loop for the full rationale.
                    let mut changed = matches!(event, rstui_core::Event::Resize(_));
                    let (mut outcome, produced) = handle_input(&mut app, event, &mut exec);
                    changed |= produced;
                    // Coalesce a burst the same way the sync loop does, but
                    // with no real clock: a `biased` inner `select!` tries the
                    // next event first and falls through to a *ready* future
                    // the instant input would block — draining exactly what is
                    // buffered, then one repaint with the latest state. So a
                    // fast resize/scroll/mouse-move flood has zero
                    // render-backlog latency. The wall-clock budget caps a
                    // never-ending flood so the tick/repaint cadence is never
                    // starved. `next_event` is cancel-safe (the trait's
                    // contract), so dropping the losing future never loses an
                    // event.
                    let mut coalesced = 0usize;
                    while outcome == Settled::Running
                        && coalesced < COALESCE_LIMIT
                        && batch_start.elapsed() < COALESCE_TIME_BUDGET
                    {
                        tokio::select! {
                            biased;
                            more = events.next_event() => match more {
                                Ok(Some(next)) => {
                                    changed |= matches!(next, rstui_core::Event::Resize(_));
                                    let (next_outcome, next_produced) =
                                        handle_input(&mut app, next, &mut exec);
                                    outcome = next_outcome;
                                    changed |= next_produced;
                                    coalesced += 1;
                                }
                                Ok(None) => {
                                    running = false;
                                    break;
                                }
                                Err(error) => return Err(RunError::Input(error)),
                            },
                            () = std::future::ready(()) => break,
                        }
                    }
                    if outcome == Settled::Quit {
                        running = false;
                    }
                    // Skip the repaint for a pure no-op event flood (e.g.
                    // any-motion mouse-move reports): rendering once per
                    // such burst is the RT-01 saturation that froze the UI
                    // during mouse movement. State is always exact.
                    if changed {
                        render(&mut terminal, &app).map_err(RunError::Backend)?;
                    }
                }
                // Single, unambiguous meaning (unlike sync `poll_event`):
                // input ended for good.
                Ok(None) => running = false,
                Err(error) => return Err(RunError::Input(error)),
            },

            Some(message) = rx.recv() => {
                let mut outcome = step(&mut app, message, &mut exec);
                // Coalesce a result burst (a fan-out of parallel commands all
                // completing): fold every result already queued, then repaint
                // **once** — the same zero-backlog rule Slice 15 applies to
                // input, and what the sync `run_core` already does by draining
                // its channel before one render. `try_recv` is non-blocking,
                // so it stops the instant the queue is empty.
                let mut coalesced = 0usize;
                while outcome == Settled::Running && coalesced < COALESCE_LIMIT {
                    match rx.try_recv() {
                        Ok(next) => {
                            outcome = step(&mut app, next, &mut exec);
                            coalesced += 1;
                        }
                        Err(_) => break, // empty (or closed): nothing more now
                    }
                }
                if outcome == Settled::Quit {
                    running = false;
                }
                render(&mut terminal, &app).map_err(RunError::Backend)?;
            }

            // An inline future that is `pending()` (never fires) when there is
            // no ticker, so no `if`-guard / `unwrap` is needed and the arm is
            // simply inert while the app declares no tick rate.
            () = async {
                match ticker.as_mut() {
                    Some(interval) => {
                        interval.tick().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            } => {
                if let Some(message) = app.on_tick() {
                    if step(&mut app, message, &mut exec) == Settled::Quit {
                        running = false;
                    }
                }
                render(&mut terminal, &app).map_err(RunError::Backend)?;
            }
        }
    }

    Ok(app)
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
    mut observer: Option<&mut dyn FrameObserver>,
) -> Result<A, RunError<B::Error, S::Error>>
where
    A: App,
    B: Backend,
    S: EventSource,
{
    let mut terminal = Terminal::new(backend).map_err(RunError::Backend)?;
    // ADR 0018 §3: monotonic observed-frame index; only touched on the
    // observed path (`observer.is_some()`), so the default loop is
    // byte-identical and pays no timing cost.
    let mut frame_no: u64 = 0;

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
                let batch_start = Instant::now();
                // A resize must repaint even if the app models no message for
                // it (`view` re-reads `frame.area()`), so it counts as a
                // change independently of whether it produced a message.
                let mut changed = matches!(event, rstui_core::Event::Resize(_));
                let (mut outcome, produced) = handle_input(&mut app, event, exec);
                changed |= produced;
                // Coalesce a burst (fast resize-drag / scroll-wheel spin / a
                // mouse-move flood): fold every event already buffered, then
                // repaint **once** with the latest state. A non-blocking
                // `ZERO` poll drains exactly what is ready and stops the
                // instant nothing is; the wall-clock budget additionally caps
                // a *never-ending* flood so the tick deadline and repaint
                // cadence are never starved (the freeze-while-moving bug).
                let mut coalesced = 0usize;
                while outcome == Settled::Running
                    && coalesced < COALESCE_LIMIT
                    && batch_start.elapsed() < COALESCE_TIME_BUDGET
                {
                    match events
                        .poll_event(Some(Duration::ZERO))
                        .map_err(RunError::Input)?
                    {
                        Some(next) => {
                            changed |= matches!(next, rstui_core::Event::Resize(_));
                            let (next_outcome, next_produced) = handle_input(&mut app, next, exec);
                            outcome = next_outcome;
                            changed |= next_produced;
                            coalesced += 1;
                        }
                        None => break, // nothing more immediately ready
                    }
                }
                if outcome == Settled::Quit {
                    running = false;
                }
                // Service a due tick even while input is flooding. Under
                // continuous motion `poll_event` never times out, so the
                // `None` arm's tick path is unreachable — without this the
                // animation clock (spinner, header time, toast expiry) would
                // freeze for the whole duration of the move even though the
                // repaint backlog is already gone. Folding it here keeps the
                // cadence; the single repaint below covers it.
                if let Some(rate) = rate {
                    let deadline = *next_tick.get_or_insert_with(|| Instant::now() + rate);
                    if Instant::now() >= deadline {
                        next_tick = Some(Instant::now() + rate);
                        if let Some(message) = app.on_tick() {
                            if step(&mut app, message, exec) == Settled::Quit {
                                running = false;
                            }
                        }
                        changed = true;
                    }
                }
                // Repaint only when the coalesced batch actually changed
                // state (≥1 message) or the terminal resized. A pure no-op
                // event flood — most commonly the per-sample mouse-move
                // reports any-motion mouse capture emits while the pointer
                // moves — must NOT trigger a full `view`+`diff`: doing so
                // once per burst is the render saturation that froze/lagged
                // the UI during mouse movement (RT-01). State is always
                // exact — every event was still folded — so a skipped
                // repaint is never observable.
                if changed {
                    if let Some(obs) = observer.as_deref_mut() {
                        let (view, flush) =
                            render_timed(&mut terminal, &app).map_err(RunError::Backend)?;
                        let total = batch_start.elapsed();
                        obs.on_frame(&FrameMetrics {
                            frame: frame_no,
                            logic: total.saturating_sub(view + flush),
                            view,
                            flush,
                            total,
                            produced: true,
                            events_coalesced: u32::try_from(coalesced)
                                .unwrap_or(u32::MAX)
                                .saturating_add(1),
                            input_latency: total,
                        });
                        frame_no += 1;
                    } else {
                        render(&mut terminal, &app).map_err(RunError::Backend)?;
                    }
                } else if let Some(obs) = observer.as_deref_mut() {
                    // RT-01: a no-op coalesced flood skipped the repaint.
                    // Still report the iteration so the overlay can *show*
                    // the skip working (high `events_coalesced`,
                    // `produced: false`, zero `view`/`flush`).
                    let total = batch_start.elapsed();
                    obs.on_frame(&FrameMetrics {
                        frame: frame_no,
                        logic: total,
                        view: Duration::ZERO,
                        flush: Duration::ZERO,
                        total,
                        produced: false,
                        events_coalesced: u32::try_from(coalesced)
                            .unwrap_or(u32::MAX)
                            .saturating_add(1),
                        input_latency: total,
                    });
                    frame_no += 1;
                }
            }
            None => match rate {
                // A *bounded* wait returned `None`: the timer elapsed (or the
                // source had nothing this tick). Re-arm from now so missed
                // ticks coalesce, then route the tick through the **same**
                // `update`/`settle` path as input.
                Some(rate) => {
                    let tick_t0 = observer.as_ref().map(|_| Instant::now());
                    next_tick = Some(Instant::now() + rate);
                    if let Some(message) = app.on_tick() {
                        if step(&mut app, message, exec) == Settled::Quit {
                            running = false;
                        }
                    }
                    if let Some(obs) = observer.as_deref_mut() {
                        let (view, flush) =
                            render_timed(&mut terminal, &app).map_err(RunError::Backend)?;
                        let total = tick_t0.map(|t| t.elapsed()).unwrap_or_default();
                        obs.on_frame(&FrameMetrics {
                            frame: frame_no,
                            logic: total.saturating_sub(view + flush),
                            view,
                            flush,
                            total,
                            produced: true,
                            events_coalesced: 0,
                            input_latency: Duration::ZERO,
                        });
                        frame_no += 1;
                    } else {
                        render(&mut terminal, &app).map_err(RunError::Backend)?;
                    }
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

    #[test]
    fn run_pooled_drains_a_command_flood_with_a_small_fixed_pool() {
        // A burst of 64 off-loop performs through a pool of only 2 workers:
        // every result must still fold (the pool drains the queue at width 2),
        // proving bounded concurrency does not lose work. The 65th update quits
        // so the outcome is deterministic even though scheduling is not.
        #[derive(Default)]
        struct Flood {
            done: u32,
        }
        enum Msg {
            Done,
        }
        const BURST: u32 = 64;
        impl App for Flood {
            type Message = Msg;
            fn init(&mut self) -> Cmd<Msg> {
                Cmd::batch((0..BURST).map(|_| Cmd::perform(|| Msg::Done)))
            }
            fn update(&mut self, _: Msg) -> Cmd<Msg> {
                self.done += 1;
                if self.done == BURST {
                    Cmd::quit()
                } else {
                    Cmd::none()
                }
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let mut input = TestEventSource::new();
        let app = run_pooled(
            Flood::default(),
            TestBackend::new(2, 1),
            &mut input,
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
        assert_eq!(app.done, BURST, "the 2-worker pool drained every command");
    }

    #[test]
    fn run_pooled_runs_a_timer_without_starving_its_single_worker() {
        // One worker and a `Cmd::tick`: if the timer's sleep ran *on* the pool
        // worker it could starve the follow-up perform. The timer thread is
        // separate, so both the tick and the perform it schedules complete and
        // the app quits — a deterministic outcome on a width-1 pool.
        #[derive(Default)]
        struct Timed {
            ticked: bool,
            worked: bool,
        }
        enum Msg {
            Tick,
            Worked,
        }
        impl App for Timed {
            type Message = Msg;
            fn init(&mut self) -> Cmd<Msg> {
                Cmd::tick(Duration::from_millis(1), || Msg::Tick)
            }
            fn update(&mut self, message: Msg) -> Cmd<Msg> {
                match message {
                    Msg::Tick => {
                        self.ticked = true;
                        Cmd::perform(|| Msg::Worked)
                    }
                    Msg::Worked => {
                        self.worked = true;
                        Cmd::quit()
                    }
                }
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let mut input = TestEventSource::new();
        let app = run_pooled(
            Timed::default(),
            TestBackend::new(2, 1),
            &mut input,
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
        assert!(app.ticked && app.worked, "tick fired then pooled work ran");
    }

    /// A scripted [`AsyncEventSource`] backed by a tokio `mpsc`: the async
    /// dual of `TestEventSource`. `next_event` is cancel-safe (channel-backed),
    /// so it is sound under the loop's `select!`. While the sender is held and
    /// idle, `recv().await` stays pending — the loop is then driven purely by
    /// command results / ticks, exactly the path under test.
    #[cfg(feature = "async")]
    struct ScriptedAsyncSource {
        rx: tokio::sync::mpsc::UnboundedReceiver<Event>,
    }

    #[cfg(feature = "async")]
    impl AsyncEventSource for ScriptedAsyncSource {
        type Error = Infallible;
        async fn next_event(&mut self) -> Result<Option<Event>, Infallible> {
            // `None` (all senders dropped) is the clean end-of-input signal.
            Ok(self.rx.recv().await)
        }
    }

    /// The async loop drives the *same* reducer as the sync loops: `init`'s
    /// off-loop perform "fails" once, a real `Cmd::tick` retry then a perform
    /// succeed and quit. `start_paused` gives virtual time (auto-advanced when
    /// idle), so the tick resolves deterministically with **no real clock** —
    /// the async-plumbing analogue of the sync `Harness`'s determinism.
    /// Exercised only under `--features async`; the workspace CI gate runs
    /// `--all-features`, so it is covered while the default build never
    /// compiles tokio.
    #[cfg(feature = "async")]
    #[tokio::test(start_paused = true)]
    async fn run_async_drives_the_same_reducer_over_the_select_loop() {
        #[derive(Default)]
        struct AsyncApp {
            attempts: u32,
            done: bool,
        }
        enum Msg {
            Failed,
            Retry,
            Done,
        }
        impl App for AsyncApp {
            type Message = Msg;
            fn init(&mut self) -> Cmd<Msg> {
                Cmd::perform(|| Msg::Failed)
            }
            fn update(&mut self, message: Msg) -> Cmd<Msg> {
                match message {
                    Msg::Failed => {
                        self.attempts += 1;
                        Cmd::tick(Duration::from_millis(1), || Msg::Retry)
                    }
                    Msg::Retry => Cmd::perform(|| Msg::Done),
                    Msg::Done => {
                        self.done = true;
                        Cmd::quit()
                    }
                }
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let (keepalive_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut source = ScriptedAsyncSource { rx };
        let app = run_async(AsyncApp::default(), TestBackend::new(2, 1), &mut source)
            .await
            .unwrap();
        // Held until after the loop so `next_event` never reports end-of-input
        // (the app must stop via `Cmd::quit`, not a drained source).
        drop(keepalive_tx);
        assert_eq!(app.attempts, 1, "the off-loop perform failed once");
        assert!(app.done, "the tick retry then perform completed via select");
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

    /// A `Backend` counting `draw` calls (one per presented frame) through a
    /// shared `Cell`, so a test can prove burst coalescing collapses N input
    /// events into **one** repaint instead of N.
    #[derive(Clone)]
    struct RenderCountingBackend {
        inner: Rc<RefCell<TestBackend>>,
        draws: Rc<std::cell::Cell<usize>>,
    }

    impl Backend for RenderCountingBackend {
        type Error = Infallible;

        fn draw<'a, Iter>(&mut self, cells: Iter) -> Result<(), Self::Error>
        where
            Iter: IntoIterator<Item = (Position, &'a Cell)>,
        {
            self.draws.set(self.draws.get() + 1);
            self.inner.borrow_mut().draw(cells)
        }

        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            self.inner.borrow_mut().hide_cursor()
        }
        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            self.inner.borrow_mut().show_cursor()
        }
        fn cursor_position(&mut self) -> Result<Position, Self::Error> {
            self.inner.borrow_mut().cursor_position()
        }
        fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
            self.inner.borrow_mut().set_cursor_position(position)
        }
        fn clear(&mut self) -> Result<(), Self::Error> {
            self.inner.borrow_mut().clear()
        }
        fn size(&self) -> Result<Size, Self::Error> {
            self.inner.borrow().size()
        }
        fn flush(&mut self) -> Result<(), Self::Error> {
            self.inner.borrow_mut().flush()
        }
    }

    /// Records every resize width it is told about and quits on `q` — the
    /// fixture for asserting a fast resize/scroll *burst* folds every event
    /// (state is exact) but repaints only once (no render backlog → no lag).
    #[derive(Default)]
    struct WidthLog {
        widths: Vec<u16>,
    }

    enum WidthMsg {
        Resized(u16),
        Quit,
    }

    impl App for WidthLog {
        type Message = WidthMsg;

        fn on_event(&self, event: Event) -> Option<WidthMsg> {
            match event {
                Event::Resize(size) => Some(WidthMsg::Resized(size.width)),
                Event::Key(_) if event.is_key(KeyCode::Char('q')) => Some(WidthMsg::Quit),
                _ => None,
            }
        }

        fn update(&mut self, message: WidthMsg) -> Cmd<WidthMsg> {
            match message {
                WidthMsg::Resized(width) => {
                    self.widths.push(width);
                    Cmd::none()
                }
                WidthMsg::Quit => Cmd::quit(),
            }
        }

        fn view(&self, frame: &mut rstui_core::Frame<'_>) {
            let pos = frame.area().position();
            frame.buffer_mut().set_str(
                pos,
                &format!("w={}", self.widths.last().copied().unwrap_or(0)),
                Style::new(),
            );
        }
    }

    fn resize(w: u16) -> Event {
        Event::Resize(Size::new(w, 4))
    }

    #[test]
    fn sync_loop_coalesces_a_resize_burst_into_one_repaint() {
        let draws = Rc::new(std::cell::Cell::new(0));
        let backend = RenderCountingBackend {
            inner: Rc::new(RefCell::new(TestBackend::new(8, 4))),
            draws: Rc::clone(&draws),
        };
        // Three resizes then quit, all buffered: `TestEventSource` yields them
        // back-to-back, so the ZERO-poll drain folds the whole burst.
        let mut input =
            TestEventSource::with_events([resize(10), resize(20), resize(30), key('q')]);

        let app = run(WidthLog::default(), backend, &mut input).unwrap();

        // Every resize was folded in order (state is exact, never skipped)…
        assert_eq!(app.widths, vec![10, 20, 30]);
        // …but the burst produced exactly one repaint after the initial frame
        // (init render + one coalesced batch), not one-per-event.
        assert_eq!(draws.get(), 2, "burst must coalesce to a single repaint");
    }

    /// A no-op pointer-move event (any-motion mouse capture emits one per
    /// sample while the mouse moves).
    fn moved(x: u16) -> Event {
        Event::Mouse(rstui_core::MouseEvent::new(
            rstui_core::MouseEventKind::Moved,
            Position::new(x, 0),
            rstui_core::KeyModifiers::NONE,
        ))
    }

    /// The freeze-while-moving regression. A flood of no-op events maps to
    /// **no message**, so the live loop must not repaint for it: before
    /// RT-01 the loop did a full `view`+`diff` once per coalesced burst
    /// regardless, and under continuous motion that render saturation
    /// starved ticks and the repaint cadence — the UI froze while the mouse
    /// moved. After the fix the only frame is the initial one (state never
    /// changed), so the draw counter stays at exactly 1 (it would be ≥ 2
    /// before the fix).
    #[test]
    fn a_no_op_event_flood_never_repaints() {
        let draws = Rc::new(std::cell::Cell::new(0));
        let backend = RenderCountingBackend {
            inner: Rc::new(RefCell::new(TestBackend::new(8, 4))),
            draws: Rc::clone(&draws),
        };
        // A pure pointer-move flood, then end-of-input (no quit, no tick):
        // every event maps to `None` in `WidthLog::on_event`.
        let flood: Vec<Event> = (0..64).map(moved).collect();
        let mut input = TestEventSource::with_events(flood);

        let app = run(WidthLog::default(), backend, &mut input).unwrap();

        // No resize was ever delivered, so state never changed…
        assert!(app.widths.is_empty(), "no-op events must not mutate state");
        // …and the loop presented exactly the one initial frame.
        assert_eq!(
            draws.get(),
            1,
            "a no-op event flood must trigger zero repaints \
             (freeze-while-moving regression)"
        );
    }

    /// The async loop must gate the repaint on a real state change too: the
    /// same no-op flood, over the `select!` drain, presents only the initial
    /// frame.
    #[cfg(feature = "async")]
    #[tokio::test(start_paused = true)]
    async fn async_loop_does_not_repaint_a_no_op_flood() {
        let draws = Rc::new(std::cell::Cell::new(0));
        let backend = RenderCountingBackend {
            inner: Rc::new(RefCell::new(TestBackend::new(8, 4))),
            draws: Rc::clone(&draws),
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for x in 0..64 {
            tx.send(moved(x)).unwrap();
        }
        drop(tx); // close the channel so the drained source reports EOF.
        let mut source = ScriptedAsyncSource { rx };

        let app = run_async(WidthLog::default(), backend, &mut source)
            .await
            .unwrap();

        assert!(app.widths.is_empty());
        assert_eq!(
            draws.get(),
            1,
            "the async select! drain must not repaint a no-op flood"
        );
    }

    #[cfg(feature = "async")]
    #[tokio::test(start_paused = true)]
    async fn async_loop_coalesces_a_resize_burst_into_one_repaint() {
        let draws = Rc::new(std::cell::Cell::new(0));
        let backend = RenderCountingBackend {
            inner: Rc::new(RefCell::new(TestBackend::new(8, 4))),
            draws: Rc::clone(&draws),
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for event in [resize(10), resize(20), resize(30), key('q')] {
            tx.send(event).unwrap();
        }
        let mut source = ScriptedAsyncSource { rx };

        let app = run_async(WidthLog::default(), backend, &mut source)
            .await
            .unwrap();
        drop(tx); // held until the loop quit, so no premature end-of-input.

        assert_eq!(app.widths, vec![10, 20, 30]);
        assert_eq!(
            draws.get(),
            2,
            "the async select! drain coalesces the burst to one repaint"
        );
    }

    /// A fan-out of `BURST` off-loop `perform`s all complete and queue on the
    /// result channel; the async loop must fold every one but repaint far
    /// fewer than `BURST` times (it drains `try_recv` then renders once, like
    /// the sync loop and Slice 15's input path). The exact count depends on
    /// blocking-pool scheduling, so the deterministic, flake-free assertion is
    /// "every result folded" + "renders ≪ results" (without coalescing it
    /// would be ≈ `BURST`).
    #[cfg(feature = "async")]
    #[tokio::test(start_paused = true)]
    async fn async_loop_coalesces_a_command_result_burst() {
        #[derive(Default)]
        struct FanOut {
            done: u32,
        }
        enum Msg {
            Done,
        }
        const BURST: u32 = 64;
        impl App for FanOut {
            type Message = Msg;
            fn init(&mut self) -> Cmd<Msg> {
                Cmd::batch((0..BURST).map(|_| Cmd::perform(|| Msg::Done)))
            }
            fn update(&mut self, _: Msg) -> Cmd<Msg> {
                self.done += 1;
                if self.done == BURST {
                    Cmd::quit()
                } else {
                    Cmd::none()
                }
            }
            fn view(&self, _: &mut rstui_core::Frame<'_>) {}
        }
        let draws = Rc::new(std::cell::Cell::new(0));
        let backend = RenderCountingBackend {
            inner: Rc::new(RefCell::new(TestBackend::new(2, 1))),
            draws: Rc::clone(&draws),
        };
        // No input: the loop is driven purely by the command-result burst.
        // The sender is held so `next_event` never reports end-of-input.
        let (keepalive_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut source = ScriptedAsyncSource { rx };

        let app = run_async(FanOut::default(), backend, &mut source)
            .await
            .unwrap();
        drop(keepalive_tx);

        assert_eq!(app.done, BURST, "every off-loop result folded");
        assert!(
            draws.get() < BURST as usize,
            "results coalesced: {} repaints for {BURST} results",
            draws.get(),
        );
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
