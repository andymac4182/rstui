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
//! [`Harness`](crate::Harness) runs it inline and never needs `Send`, but the
//! real terminal runtime will run commands off the render loop (a thread pool
//! or async executor), exactly as Bubble Tea runs commands on goroutines.
//! Requiring it now keeps that future seam from being a breaking change.

use std::fmt;

/// One unit of work the runtime performs on the app's behalf.
enum Effect<M> {
    /// Stop the program after this command settles.
    Quit,
    /// Run this closure, then feed its message back into
    /// [`App::update`](crate::App::update).
    Perform(Box<dyn FnOnce() -> M + Send + 'static>),
}

impl<M> fmt::Debug for Effect<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quit => f.write_str("Quit"),
            Self::Perform(_) => f.write_str("Perform(..)"),
        }
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

    /// Drains the command's effects in order.
    ///
    /// Crate-internal: only the runtime performs effects. `quit` is reported by
    /// invoking `on_quit` and consumes the rest of the queue (callers stop
    /// draining), while each `perform` closure's message is handed to
    /// `on_message`.
    pub(crate) fn drain(self, mut on_message: impl FnMut(M), mut on_quit: impl FnMut()) {
        for effect in self.effects {
            match effect {
                Effect::Quit => {
                    on_quit();
                    break;
                }
                Effect::Perform(work) => on_message(work()),
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

    #[test]
    fn batch_preserves_order_and_flattens() {
        let cmd: Cmd<u8> = Cmd::batch([
            Cmd::message(1),
            Cmd::batch([Cmd::message(2), Cmd::message(3)]),
            Cmd::quit(),
        ]);
        assert_eq!(cmd.len(), 4);

        let mut seen = Vec::new();
        let mut quit = false;
        cmd.drain(|m| seen.push(m), || quit = true);
        assert_eq!(seen, vec![1, 2, 3]);
        assert!(quit);
    }

    #[test]
    fn drain_stops_at_quit() {
        // The message after the quit must never be delivered.
        let cmd: Cmd<u8> = Cmd::batch([Cmd::quit(), Cmd::message(9)]);
        let mut seen = Vec::new();
        let mut quits = 0;
        cmd.drain(|m| seen.push(m), || quits += 1);
        assert!(seen.is_empty());
        assert_eq!(quits, 1);
    }

    #[test]
    fn perform_runs_the_closure_when_drained() {
        let cmd = Cmd::perform(|| 40 + 2);
        let mut got = None;
        cmd.drain(|m| got = Some(m), || {});
        assert_eq!(got, Some(42));
    }

    #[test]
    fn debug_names_effects_without_invoking_them() {
        let cmd: Cmd<u8> = Cmd::batch([Cmd::perform(|| 1), Cmd::quit()]);
        assert_eq!(format!("{cmd:?}"), "Cmd { effects: [Perform(..), Quit] }");
    }
}
