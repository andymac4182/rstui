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
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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

/// An [`EventSource`] fed by another thread over a [`std::sync::mpsc`] channel.
///
/// Where [`TestEventSource`] proves the trait works *without* a thread, this
/// proves it works *with* one and stays a faithful, dependency-free second
/// production-shaped implementation — the property [ADR 0006] keeps open by
/// making input a trait rather than hard-wiring crossterm. The loop owns the
/// [`Receiver`] half; any number of [`Sender`]s (they are [`Clone`]) live on
/// other threads — a background `Cmd` worker, an IPC reader, a test driver —
/// and hand [`Event`]s in without the loop knowing or caring who produced them.
///
/// It maps the two `poll_event` timeout modes straight onto the two channel
/// receive primitives, so the [`EventSource::poll_event`] contract holds with
/// no extra bookkeeping:
///
/// - `None` (block until input or end) → [`Receiver::recv`]. An [`Event`]
///   yields `Ok(Some)`. `Ok(None)` is returned **only** once every [`Sender`]
///   has been dropped: the channel can never produce again, which is exactly
///   *permanent* end-of-input — the unbounded `Ok(None)` the loop stops on,
///   the analog of a closed terminal or a drained [`TestEventSource`].
/// - `Some(d)` (wait at most `d`) → [`Receiver::recv_timeout`]. An [`Event`]
///   yields `Ok(Some)`; the deadline elapsing yields `Ok(None)` — a
///   *transient* miss the caller loops past. A channel closed *before* the
///   deadline also yields `Ok(None)`: a timed caller correctly reads any
///   `None` as "nothing this round" and polls again, the same conflation
///   [`TestEventSource`] documents and the contract sanctions (the two
///   meanings of `Ok(None)` are disambiguated by the *caller's* timeout, not
///   the source).
///
/// Like [`TestEventSource`] a closed channel is **not** an error: it is
/// end-of-input, so [`Error`](EventSource::Error) is [`Infallible`] and the
/// loop never has to distinguish "input stopped" from "input failed".
///
/// # Example
///
/// ```
/// use std::thread;
///
/// use rstui_core::event::{Event, KeyEvent};
/// use rstui_core::event_source::{ChannelEventSource, EventSource};
///
/// let (mut input, tx) = ChannelEventSource::new();
///
/// // A producer thread feeds two events, then drops its sender by returning.
/// let producer = thread::spawn(move || {
///     tx.send(Event::from(KeyEvent::char('h'))).unwrap();
///     tx.send(Event::from(KeyEvent::char('i'))).unwrap();
/// });
///
/// // The loop blocks for each event, then sees end-of-input once the only
/// // sender is gone — the unbounded `Ok(None)` it stops on.
/// assert_eq!(input.poll_event(None).unwrap(), Some(Event::from(KeyEvent::char('h'))));
/// assert_eq!(input.poll_event(None).unwrap(), Some(Event::from(KeyEvent::char('i'))));
/// producer.join().unwrap();
/// assert_eq!(input.poll_event(None).unwrap(), None);
/// ```
///
/// [ADR 0006]: https://github.com/andymac4182/rstui/blob/main/docs/adr/0006-runtime-event-loop.md
#[derive(Debug)]
pub struct ChannelEventSource {
    receiver: Receiver<Event>,
}

impl ChannelEventSource {
    /// Creates a source and the first [`Sender`] that feeds it.
    ///
    /// The sender is returned *alongside* the source rather than reachable
    /// only through it because the producer is on another thread: it must own
    /// a sending handle the loop's `&mut self` could never lend out. Clone the
    /// returned [`Sender`] for additional producers; the source ends only when
    /// the last clone drops (see the type docs).
    #[must_use]
    pub fn new() -> (Self, Sender<Event>) {
        let (sender, receiver) = mpsc::channel();
        (Self { receiver }, sender)
    }

    /// Wraps a pre-existing [`Receiver`], for a caller that built the channel
    /// itself (e.g. it already shared the [`Sender`] before constructing the
    /// loop). Equivalent to keeping the sender from [`new`](Self::new).
    #[must_use]
    pub fn from_receiver(receiver: Receiver<Event>) -> Self {
        Self { receiver }
    }
}

impl EventSource for ChannelEventSource {
    type Error = Infallible;

    fn poll_event(&mut self, timeout: Option<Duration>) -> Result<Option<Event>, Self::Error> {
        match timeout {
            // Unbounded: block until an event arrives or every sender is gone.
            // `recv` erroring means the channel is closed for good — the
            // *permanent* end-of-input `Ok(None)`, not a failure.
            None => Ok(self.receiver.recv().ok()),
            // Bounded: an event, or `None` for either a timeout (transient) or
            // a closed channel. A timed caller treats every `None` as "nothing
            // yet" and polls again, so collapsing both into `Ok(None)` is
            // exactly the contract (and matches `TestEventSource`).
            Some(duration) => match self.receiver.recv_timeout(duration) {
                Ok(event) => Ok(Some(event)),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => Ok(None),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent};
    use std::thread;

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

    #[test]
    fn channel_blocking_poll_yields_sent_events_in_order() {
        let (mut src, tx) = ChannelEventSource::new();
        tx.send(key('a')).unwrap();
        tx.send(key('b')).unwrap();

        // Already queued, so an unbounded poll returns each at once, in order.
        assert_eq!(src.poll_event(None).unwrap(), Some(key('a')));
        assert_eq!(src.poll_event(None).unwrap(), Some(key('b')));
    }

    #[test]
    fn channel_unbounded_poll_blocks_until_a_producer_sends() {
        // Proves `poll_event(None)` genuinely *waits* for input rather than
        // busy-returning `None`: the event is sent only from another thread.
        let (mut src, tx) = ChannelEventSource::new();
        let producer = thread::spawn(move || {
            tx.send(key('z')).unwrap();
        });

        assert_eq!(src.poll_event(None).unwrap(), Some(key('z')));
        producer.join().unwrap();
    }

    #[test]
    fn channel_dropping_every_sender_is_unbounded_end_of_input() {
        let (mut src, tx) = ChannelEventSource::new();
        tx.send(key('q')).unwrap();
        drop(tx);

        // Buffered events still drain first, *then* the closed channel is the
        // permanent `Ok(None)` an unbounded caller stops on (the contract's
        // terminal signal, like a drained `TestEventSource`).
        assert_eq!(src.poll_event(None).unwrap(), Some(key('q')));
        assert_eq!(src.poll_event(None).unwrap(), None);
        assert_eq!(src.poll_event(None).unwrap(), None);
    }

    #[test]
    fn channel_bounded_poll_times_out_without_blocking_forever() {
        // No sender ever sends; a tiny timeout must elapse and return `None`
        // (a *transient* miss) rather than hang the test.
        let (mut src, _tx) = ChannelEventSource::new();
        assert_eq!(
            src.poll_event(Some(Duration::from_millis(1))).unwrap(),
            None,
        );
        // _tx is still alive, so this is a timeout, not end-of-input — yet a
        // timed caller reads either as "nothing yet" and simply polls again.
    }

    #[test]
    fn channel_bounded_poll_reads_a_closed_channel_as_nothing() {
        let (mut src, tx) = ChannelEventSource::new();
        drop(tx);
        // Closed before the deadline: a timed caller still gets `Ok(None)`
        // (the conflation the contract sanctions and `TestEventSource` shares),
        // and never blocks for the full duration.
        assert_eq!(src.poll_event(Some(Duration::from_secs(99))).unwrap(), None,);
    }

    #[test]
    fn channel_fans_in_from_multiple_cloned_senders() {
        let (mut src, tx) = ChannelEventSource::new();
        let tx2 = tx.clone();
        tx.send(key('1')).unwrap();
        tx2.send(key('2')).unwrap();

        // Both clones feed the one receiver; end-of-input waits for the *last*
        // sender, so dropping only one keeps the source live.
        assert_eq!(src.poll_event(None).unwrap(), Some(key('1')));
        drop(tx);
        assert_eq!(src.poll_event(None).unwrap(), Some(key('2')));
        drop(tx2);
        assert_eq!(src.poll_event(None).unwrap(), None);
    }

    #[test]
    fn channel_from_receiver_matches_new() {
        // `from_receiver` is just `new` with a caller-built channel: same
        // behavior, including the closed-channel end-of-input.
        let (tx, rx) = std::sync::mpsc::channel();
        let mut src = ChannelEventSource::from_receiver(rx);
        tx.send(key('k')).unwrap();
        assert_eq!(src.poll_event(None).unwrap(), Some(key('k')));
        drop(tx);
        assert_eq!(src.poll_event(None).unwrap(), None);
    }

    /// The same generic-drive proof as [`drives_an_app_loop_generically`], but
    /// over `ChannelEventSource`: one `S: EventSource` body consumes either
    /// implementation unboxed, exactly how `rstui_runtime::run` is written.
    #[test]
    fn channel_drives_an_app_loop_generically() {
        fn collect_until_end<S: EventSource>(src: &mut S) -> Vec<Event> {
            let mut seen = Vec::new();
            while let Some(event) = src.poll_event(None).expect("infallible source") {
                seen.push(event);
            }
            seen
        }

        let (mut src, tx) = ChannelEventSource::new();
        let producer = thread::spawn(move || {
            tx.send(key('1')).unwrap();
            tx.send(key('2')).unwrap();
            tx.send(Event::from(KeyEvent::from_code(KeyCode::Enter)))
                .unwrap();
            // Returning drops `tx`, closing the channel so the generic loop
            // observes end-of-input and stops.
        });

        let seen = collect_until_end(&mut src);
        producer.join().unwrap();
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
