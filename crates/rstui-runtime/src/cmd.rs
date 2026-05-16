//! Side effects an [`App`](crate::App) schedules: the `Cmd` half of the
//! Elm/Bubble Tea `update -> (state, Cmd)` contract.
//!
//! An [`App`](crate::App) never performs IO, spawns work, or quits directly.
//! Instead [`update`](crate::App::update) returns a [`Cmd`] describing *what
//! should happen next*, and the runtime performs it, feeding any resulting
//! message back into `update`. That single rule is what makes the whole
//! framework testable: a [`Harness`](crate::Harness) can run the exact same
//! commands deterministically with no terminal, threads, or clock.
//!
//! A [`Cmd`] is simply an ordered list of effects, so composition is just
//! concatenation — [`Cmd::batch`] needs no special runtime support and order is
//! always preserved.
//!
//! ```
//! use rstui_runtime::Cmd;
//!
//! // Do nothing this turn.
//! let _: Cmd<()> = Cmd::none();
//!
//! // Fold a follow-up message back into `update`, then stop the program.
//! #[derive(Debug)]
//! enum Msg {
//!     Loaded(u32),
//! }
//! let cmd = Cmd::batch([
//!     Cmd::perform(|| Msg::Loaded(7)),
//!     Cmd::quit(),
//! ]);
//! assert_eq!(cmd.len(), 2);
//! ```
//!
//! The work closure is required to be `Send + 'static`. The headless
//! [`Harness`](crate::Harness) and the default [`run`](crate::run()) loop run
//! it inline through an `InlineExecutor`; the opt-in
//! [`run_threaded`](crate::run_threaded) loop runs it off the render loop
//! through a `std::thread`-per-command `ThreadCommandExecutor`, exactly as
//! Bubble Tea runs commands on goroutines, with **no external dependency**. The
//! `Send + 'static` bound is what makes that opt-in non-breaking. Which
//! executor is in play is the *only* difference between the two loops; the
//! reducer (`settle`) is identical, so a ticking/loading app is as testable
//! under the headless harness as any other. See
//! [ADR 0008](https://github.com/andymac4182/rstui/blob/main/docs/adr/0008-async-command-executor.md).

use std::fmt;
use std::time::Duration;

/// One unit of work the runtime performs on the app's behalf.
enum Effect<M> {
    /// Stop the program after this command settles.
    Quit,
    /// Run this closure, then feed its message back into
    /// [`App::update`](crate::App::update).
    Perform(Box<dyn FnOnce() -> M + Send + 'static>),
    /// Run this closure after a delay, then feed its message back into
    /// [`App::update`](crate::App::update). `aligned` selects wall-clock
    /// alignment ([`Cmd::every`]) over a plain relative delay ([`Cmd::tick`]).
    Timer {
        delay: Duration,
        aligned: bool,
        work: Box<dyn FnOnce() -> M + Send + 'static>,
    },
}

impl<M> fmt::Debug for Effect<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quit => f.write_str("Quit"),
            Self::Perform(_) => f.write_str("Perform(..)"),
            Self::Timer { delay, aligned, .. } => {
                write!(f, "Timer {{ delay: {delay:?}, aligned: {aligned} }}")
            }
        }
    }
}

/// How the runtime *realizes* a deferred effect (a [`perform`](Cmd::perform) or
/// a [`tick`](Cmd::tick)/[`every`](Cmd::every) timer).
///
/// This is the one seam that differs between the headless/default inline loop
/// and the opt-in threaded loop, so it is the single place the
/// "run work inline now" vs. "run work off the loop, deliver later" decision
/// lives — the reducer ([`settle`](crate::run::settle)) is identical for both,
/// which is what keeps [`Harness`](crate::Harness) an exact stand-in for the
/// live loop even with async commands (ADR 0008).
///
/// Crate-internal and effectively sealed: applications never implement this.
/// They pick a behavior by calling [`run`](crate::run()) (inline) or
/// [`run_threaded`](crate::run_threaded) (off-loop); the headless
/// [`Harness`](crate::Harness) is always inline so tests stay deterministic.
pub(crate) trait CommandExecutor<M> {
    /// Realize a [`perform`](Cmd::perform). Return `Some(message)` to fold it
    /// **now** (inline); return `None` to take ownership of the work and
    /// deliver its message to the loop later (off-loop).
    fn perform(&mut self, work: Box<dyn FnOnce() -> M + Send + 'static>) -> Option<M>;

    /// Realize a timer. `delay` is the relative delay; `aligned` means snap to
    /// the next wall-clock multiple of `delay` ([`Cmd::every`]). Same
    /// `Some`/`None` contract as [`perform`](CommandExecutor::perform): an
    /// inline executor collapses the delay to zero and returns the message
    /// immediately (deterministic); an off-loop executor waits, then delivers.
    fn timer(
        &mut self,
        delay: Duration,
        aligned: bool,
        work: Box<dyn FnOnce() -> M + Send + 'static>,
    ) -> Option<M>;
}

/// The deterministic executor: runs every effect's closure **inline, now**, and
/// collapses any timer delay to zero.
///
/// Used by the headless [`Harness`](crate::Harness) and the default
/// [`run`](crate::run()) loop, so their behavior is byte-for-byte the pre-async
/// loop: a [`perform`](Cmd::perform) message re-enters `update` before the next
/// input, and a [`tick`](Cmd::tick)/[`every`](Cmd::every) fires immediately
/// with zero virtual delay. Collapsing delays to zero is exactly how
/// effect-driven test harnesses (elm-program-test, Bubble Tea's tests) resolve
/// timers: the test asserts the *post-effect* state, with no wall clock.
pub(crate) struct InlineExecutor;

impl<M> CommandExecutor<M> for InlineExecutor {
    fn perform(&mut self, work: Box<dyn FnOnce() -> M + Send + 'static>) -> Option<M> {
        Some(work())
    }

    fn timer(
        &mut self,
        _delay: Duration,
        _aligned: bool,
        work: Box<dyn FnOnce() -> M + Send + 'static>,
    ) -> Option<M> {
        // Zero virtual delay: deterministic, clock-free. The "wait `delay`"
        // semantics are honored only by the threaded executor.
        Some(work())
    }
}

/// A description of side effects to perform after an
/// [`update`](crate::App::update), returned to the runtime.
///
/// Construct one with [`none`](Cmd::none), [`quit`](Cmd::quit),
/// [`message`](Cmd::message), [`perform`](Cmd::perform), or
/// [`batch`](Cmd::batch). Commands are values: they carry no effect until the
/// runtime runs them, which is what makes update logic pure and unit-testable.
#[must_use = "a Cmd does nothing unless returned to the runtime"]
pub struct Cmd<M> {
    effects: Vec<Effect<M>>,
}

impl<M> Cmd<M> {
    /// A command that does nothing.
    ///
    /// The common return from an [`update`](crate::App::update) that only
    /// changed state and needs no follow-up work.
    pub fn none() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    /// A command that stops the program after the current command settles.
    ///
    /// Remaining effects queued behind the quit are dropped — quitting halts
    /// the loop deterministically rather than draining first.
    pub fn quit() -> Self {
        Self {
            effects: vec![Effect::Quit],
        }
    }

    /// A command that immediately feeds `message` back into
    /// [`update`](crate::App::update).
    ///
    /// Sugar for [`perform`](Cmd::perform) with a closure that just returns the
    /// message; handy for chaining a follow-up reducer step.
    pub fn message(message: M) -> Self
    where
        M: Send + 'static,
    {
        Self::perform(move || message)
    }

    /// A command that runs `work`, then feeds the message it returns back into
    /// [`update`](crate::App::update).
    ///
    /// This is the effect primitive: a future load, a computation, a tick. The
    /// closure is `Send + 'static` so the real runtime can run it off the
    /// render loop; the headless [`Harness`](crate::Harness) runs it inline.
    pub fn perform<F>(work: F) -> Self
    where
        F: FnOnce() -> M + Send + 'static,
    {
        Self {
            effects: vec![Effect::Perform(Box::new(work))],
        }
    }

    /// A command that runs `work` after `delay`, then feeds the message it
    /// returns back into [`update`](crate::App::update).
    ///
    /// The scheduled-effect timer (Bubble Tea's `tea.Tick`): a one-shot delay
    /// relative to *now*. Repeat by returning another `tick` from
    /// [`update`](crate::App::update) when its message arrives.
    ///
    /// Under [`run_threaded`](crate::run_threaded) the wait happens off the
    /// render loop, so the UI stays responsive. Under the headless
    /// [`Harness`](crate::Harness) and the default [`run`](crate::run()) the
    /// delay collapses to **zero** and the message is delivered immediately, so
    /// tests stay deterministic with no clock. This is the scheduled-`Cmd`
    /// complement to the steady [`tick_rate`](crate::App::tick_rate)
    /// *subscription*; see
    /// [ADR 0008](https://github.com/andymac4182/rstui/blob/main/docs/adr/0008-async-command-executor.md).
    pub fn tick<F>(delay: Duration, work: F) -> Self
    where
        F: FnOnce() -> M + Send + 'static,
    {
        Self {
            effects: vec![Effect::Timer {
                delay,
                aligned: false,
                work: Box::new(work),
            }],
        }
    }

    /// A command that runs `work` at the next wall-clock multiple of `period`,
    /// then feeds the message it returns back into
    /// [`update`](crate::App::update).
    ///
    /// Bubble Tea's `tea.Every`: like [`tick`](Cmd::tick) but the fire time is
    /// snapped to the system clock, so a one-second `every` fires at
    /// `…:01.000`, not one second after it was scheduled. The threaded loop
    /// computes the alignment when it dispatches the timer; the inline executor
    /// still collapses it to an immediate, deterministic delivery.
    pub fn every<F>(period: Duration, work: F) -> Self
    where
        F: FnOnce() -> M + Send + 'static,
    {
        Self {
            effects: vec![Effect::Timer {
                delay: period,
                aligned: true,
                work: Box::new(work),
            }],
        }
    }

    /// Combines several commands into one, preserving their order.
    ///
    /// Because a [`Cmd`] is just a list of effects, batching is concatenation;
    /// the runtime processes the effects first-to-last.
    pub fn batch<I>(commands: I) -> Self
    where
        I: IntoIterator<Item = Cmd<M>>,
    {
        Self {
            effects: commands.into_iter().flat_map(|cmd| cmd.effects).collect(),
        }
    }

    /// The number of effects this command will perform.
    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether this command performs no effects (a [`none`](Cmd::none)).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Dispatches the command's effects in order through `exec`.
    ///
    /// Crate-internal: only the runtime performs effects, and only through a
    /// [`CommandExecutor`] so the inline-vs-off-loop choice lives in exactly
    /// one place. `quit` is reported via `on_quit` and consumes the rest of the
    /// queue (callers stop draining). A `perform`/timer effect is handed to
    /// `exec`; the executor either runs it now and yields `Some(message)` (fed
    /// to `on_message` so it folds this turn, the inline path) or takes it
    /// off-loop and yields `None` (its message reaches the loop later).
    pub(crate) fn dispatch(
        self,
        exec: &mut dyn CommandExecutor<M>,
        mut on_message: impl FnMut(M),
        mut on_quit: impl FnMut(),
    ) {
        for effect in self.effects {
            match effect {
                Effect::Quit => {
                    on_quit();
                    break;
                }
                Effect::Perform(work) => {
                    if let Some(message) = exec.perform(work) {
                        on_message(message);
                    }
                }
                Effect::Timer {
                    delay,
                    aligned,
                    work,
                } => {
                    if let Some(message) = exec.timer(delay, aligned, work) {
                        on_message(message);
                    }
                }
            }
        }
    }
}

impl<M> Default for Cmd<M> {
    fn default() -> Self {
        Self::none()
    }
}

impl<M> fmt::Debug for Cmd<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cmd")
            .field("effects", &self.effects)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_empty_and_is_the_default() {
        let cmd: Cmd<()> = Cmd::none();
        assert!(cmd.is_empty());
        assert_eq!(cmd.len(), 0);
        assert!(Cmd::<()>::default().is_empty());
    }

    /// Dispatches `cmd` through the deterministic [`InlineExecutor`] and
    /// returns the messages it folded plus whether it quit — the shape the
    /// pre-executor `drain` tests asserted, now routed through the seam.
    fn inline(cmd: Cmd<u8>) -> (Vec<u8>, bool) {
        let mut seen = Vec::new();
        let mut quit = false;
        cmd.dispatch(&mut InlineExecutor, |m| seen.push(m), || quit = true);
        (seen, quit)
    }

    #[test]
    fn batch_preserves_order_and_flattens() {
        let cmd: Cmd<u8> = Cmd::batch([
            Cmd::message(1),
            Cmd::batch([Cmd::message(2), Cmd::message(3)]),
            Cmd::quit(),
        ]);
        assert_eq!(cmd.len(), 4);

        let (seen, quit) = inline(cmd);
        assert_eq!(seen, vec![1, 2, 3]);
        assert!(quit);
    }

    #[test]
    fn dispatch_stops_at_quit() {
        // The message after the quit must never be delivered.
        let (seen, quit) = inline(Cmd::batch([Cmd::quit(), Cmd::message(9)]));
        assert!(seen.is_empty());
        assert!(quit);
    }

    #[test]
    fn perform_runs_the_closure_under_the_inline_executor() {
        let (seen, quit) = inline(Cmd::perform(|| 40 + 2));
        assert_eq!(seen, vec![42]);
        assert!(!quit);
    }

    #[test]
    fn tick_and_every_fire_immediately_under_the_inline_executor() {
        // The deterministic contract: the inline executor collapses any timer
        // delay (relative or wall-clock-aligned) to an immediate delivery, so
        // tests never touch a clock.
        let (seen, _) = inline(Cmd::batch([
            Cmd::tick(Duration::from_secs(3600), || 1),
            Cmd::every(Duration::from_secs(60), || 2),
        ]));
        assert_eq!(seen, vec![1, 2]);
    }

    #[test]
    fn debug_names_effects_without_invoking_them() {
        let cmd: Cmd<u8> = Cmd::batch([
            Cmd::perform(|| 1),
            Cmd::tick(Duration::from_millis(5), || 2),
            Cmd::every(Duration::from_secs(1), || 3),
            Cmd::quit(),
        ]);
        assert_eq!(
            format!("{cmd:?}"),
            "Cmd { effects: [Perform(..), \
             Timer { delay: 5ms, aligned: false }, \
             Timer { delay: 1s, aligned: true }, Quit] }"
        );
    }
}
