//! The boundary between a real input device and the application loop.
//!
//! [`EventSource`] is the input dual of [`Backend`](crate::backend::Backend):
//! `Backend` is where rendered cells *go*, `EventSource` is where input
//! [`Event`]s *come from*. Keeping it a trait — exactly as `Backend` is — has
//! the same three payoffs:
//!
//! - `rstui-core` stays dependency-free; the real, non-deterministic terminal
//!   input (crossterm `poll`/`read`) lives in a backend crate, never here.
//! - The runtime is written once against the trait and runs unchanged over a
//!   real terminal or a scripted source.
//! - The deterministic test story holds: [`TestEventSource`] feeds an app a
//!   fixed event script with no TTY, the input analog of
//!   [`TestBackend`](crate::backend::TestBackend).
//!
//! rstui owns this seam (unlike ratatui, which does no input handling and
//! leaves apps calling `crossterm::event::read` directly) because core already
//! owns the [`Event`] vocabulary — a recorded, intentional divergence. The
//! same trait the future async `EventStream` path satisfies, so the
//! synchronous loop never needs async to exist.
//!
//! # Example
//!
//! ```
//! use std::time::Duration;
//!
//! use rstui_core::event::{Event, KeyCode, KeyEvent};
//! use rstui_core::event_source::{EventSource, TestEventSource};
//!
//! // A scripted source drives an app with no terminal — the input analog of
//! // `TestBackend` driving rendering with no terminal.
//! let mut input = TestEventSource::with_events([
//!     Event::from(KeyEvent::char('h')),
//!     Event::from(KeyEvent::from_code(KeyCode::Enter)),
//! ]);
//!
//! // The timeout is honored by real sources; the scripted one never blocks.
//! assert_eq!(
//!     input.poll_event(Some(Duration::ZERO)).unwrap(),
//!     Some(Event::from(KeyEvent::char('h'))),
//! );
//! assert_eq!(
//!     input.poll_event(None).unwrap(),
//!     Some(Event::from(KeyEvent::from_code(KeyCode::Enter))),
//! );
//! // Script exhausted: a `None`-timeout caller reads this as end-of-input.
//! assert_eq!(input.poll_event(None).unwrap(), None);
//! ```

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::Duration;

use crate::event::Event;

/// A source of input [`Event`]s for a running application.
///
/// The single method, [`poll_event`](EventSource::poll_event), folds
/// crossterm's `poll`-then-`read` into one timed call — the shape a render
/// loop actually wants: block for input, but optionally wake to do periodic
/// work (animation ticks, draining results a background [`Cmd`] delivered).
///
/// Like [`Backend`](crate::backend::Backend) the trait is monomorphized over
/// one concrete source rather than boxed; it carries an associated
/// [`Error`](EventSource::Error) for the same reason (`io::Error` for a real
/// terminal, [`Infallible`] for [`TestEventSource`]).
///
/// [`Cmd`]: https://docs.rs/rstui-runtime
pub trait EventSource {
    /// How this source reports failure.
    ///
    /// In-memory sources use [`Infallible`]; a real terminal source would use
    /// [`std::io::Error`].
    type Error: std::error::Error;

    /// Waits for the next input event, bounded by `timeout`.
    ///
    /// - `Some(duration)`: wait at most `duration`. `Ok(Some(event))` if one
    ///   arrived, `Ok(None)` if the wait elapsed first — a *transient* miss,
    ///   so the caller loops (perhaps doing periodic work) and polls again.
    /// - `None`: block until an event is available *or* input ends.
    ///   `Ok(Some(event))` for an event; `Ok(None)` means input is
    ///   permanently exhausted (e.g. the terminal closed) — a *terminal*
    ///   signal, so the caller stops reading.
    ///
    /// The two meanings of `Ok(None)` are disambiguated by what the caller
    /// passed: a bounded wait that returns `None` timed out; an unbounded one
    /// that returns `None` reached end-of-input.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the underlying device could not be read.
    fn poll_event(&mut self, timeout: Option<Duration>) -> Result<Option<Event>, Self::Error>;
}

/// An in-memory [`EventSource`] that replays a fixed event script.
///
/// The input analog of [`TestBackend`](crate::backend::TestBackend): it makes
/// whole apps drivable end-to-end with no terminal, no threads, and no clock.
/// Queue events (at construction or later), then let the loop poll them out in
/// order; once the script is exhausted it yields `Ok(None)`.
///
/// It deliberately **ignores `timeout`** — a deterministic source must never
/// block a test. A drained source returning `Ok(None)` is read by a
/// `None`-timeout caller as end-of-input and by a timed caller as "nothing
/// yet", both correct per the [`EventSource::poll_event`] contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestEventSource {
    queue: VecDeque<Event>,
}

impl TestEventSource {
    /// An empty source. It is immediately drained, so the first poll yields
    /// `Ok(None)`; [`push`](TestEventSource::push) events to script it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A source preloaded with `events`, replayed in iteration order.
    #[must_use]
    pub fn with_events<I>(events: I) -> Self
    where
        I: IntoIterator<Item = Event>,
    {
        Self {
            queue: events.into_iter().collect(),
        }
    }

    /// Appends one event to the end of the script.
    ///
    /// Pushing onto a drained source revives it: the next poll yields the new
    /// event rather than `Ok(None)`.
    pub fn push(&mut self, event: Event) {
        self.queue.push_back(event);
    }

    /// Appends `events` to the end of the script, in iteration order.
    pub fn extend<I>(&mut self, events: I)
    where
        I: IntoIterator<Item = Event>,
    {
        self.queue.extend(events);
    }

    /// How many scripted events remain unread.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Whether the script is exhausted (the next poll would yield `Ok(None)`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl EventSource for TestEventSource {
    type Error = Infallible;

    fn poll_event(&mut self, _timeout: Option<Duration>) -> Result<Option<Event>, Self::Error> {
        Ok(self.queue.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent};

    fn key(c: char) -> Event {
        Event::from(KeyEvent::char(c))
    }

    #[test]
    fn replays_scripted_events_in_order_then_drains() {
        let mut src = TestEventSource::with_events([key('a'), key('b')]);
        assert_eq!(src.len(), 2);
        assert!(!src.is_empty());

        assert_eq!(src.poll_event(None).unwrap(), Some(key('a')));
        assert_eq!(src.poll_event(None).unwrap(), Some(key('b')));

        // Exhausted: stays drained on repeat polls.
        assert_eq!(src.poll_event(None).unwrap(), None);
        assert_eq!(src.poll_event(None).unwrap(), None);
        assert!(src.is_empty());
    }

    #[test]
    fn timeout_is_ignored_so_the_source_is_deterministic() {
        let mut src = TestEventSource::with_events([key('x')]);
        // Bounded, zero, and unbounded waits all behave identically.
        assert_eq!(
            src.poll_event(Some(Duration::ZERO)).unwrap(),
            Some(key('x')),
        );
        assert_eq!(src.poll_event(Some(Duration::from_secs(99))).unwrap(), None);
        assert_eq!(src.poll_event(None).unwrap(), None);
    }

    #[test]
    fn default_and_new_are_empty_and_immediately_drained() {
        assert_eq!(TestEventSource::new(), TestEventSource::default());
        let mut src = TestEventSource::new();
        assert!(src.is_empty());
        assert_eq!(src.poll_event(None).unwrap(), None);
    }

    #[test]
    fn push_onto_a_drained_source_revives_it() {
        let mut src = TestEventSource::new();
        assert_eq!(src.poll_event(None).unwrap(), None);

        src.push(key('r'));
        src.extend([key('s'), key('t')]);
        assert_eq!(src.len(), 3);

        assert_eq!(src.poll_event(None).unwrap(), Some(key('r')));
        assert_eq!(src.poll_event(None).unwrap(), Some(key('s')));
        assert_eq!(src.poll_event(None).unwrap(), Some(key('t')));
        assert_eq!(src.poll_event(None).unwrap(), None);
    }

    /// Proves the trait is usable generically with no boxing — exactly how the
    /// future real `run` loop will consume it — and that `Error:
    /// std::error::Error` (so `?`/`unwrap` work uniformly across sources).
    #[test]
    fn drives_an_app_loop_generically() {
        fn collect_until_drained<S: EventSource>(src: &mut S) -> Vec<Event> {
            let mut seen = Vec::new();
            while let Some(event) = src.poll_event(None).expect("infallible source") {
                seen.push(event);
            }
            seen
        }

        let mut src = TestEventSource::with_events([key('1'), key('2')]);
        src.push(Event::from(KeyEvent::from_code(KeyCode::Enter)));

        let seen = collect_until_drained(&mut src);
        assert_eq!(
            seen,
            vec![
                key('1'),
                key('2'),
                Event::from(KeyEvent::from_code(KeyCode::Enter)),
            ],
        );
    }
}
