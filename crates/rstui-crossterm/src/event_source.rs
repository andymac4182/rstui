//! The crossterm [`EventSource`]: blocking/timed reads, unmodeled input skipped.
//!
//! [`CrosstermEventSource`] is the production side of the
//! [`EventSource`] seam `rstui-core`
//! defines — the input dual of [`CrosstermBackend`](crate::CrosstermBackend).
//! It folds crossterm's `poll`-then-`read` pair into the single timed
//! [`poll_event`](EventSource::poll_event) call the runtime loop wants, and
//! translates each native event through the already-tested
//! [`from_crossterm`] map.
//!
//! With this slice the whole framework composes end to end: the same
//! `rstui_runtime::run` the headless harness tests drive now runs an
//! unmodified [`App`](https://docs.rs/rstui-runtime) on a real terminal over a
//! [`TerminalGuard`](crate::TerminalGuard) + this source. See the
//! `run_app` example.
//!
//! # Two poll modes, two meanings of `Ok(None)`
//!
//! The [`EventSource`] contract overloads
//! `Ok(None)` and disambiguates it by the caller's `timeout`:
//!
//! - **`Some(timeout)` — a bounded wait.** Exactly one `poll`, then *at most
//!   one* `read`. It never loops: a second `read` after the single
//!   poll-guaranteed event could block past the deadline and break the timed
//!   contract. If `poll` times out, or the one event read is input rstui does
//!   not model, the result is `Ok(None)` — which a timed caller correctly reads
//!   as "nothing this tick", does its periodic work, and polls again.
//! - **`None` — block until a *modeled* event.** This path **loops**: input
//!   rstui deliberately does not model (a Kitty-only `CapsLock`, media key, …)
//!   is *skipped* and the next `read` is issued. It must **never** return
//!   `Ok(None)` here, because the runtime reads `poll_event(None) == Ok(None)`
//!   as end-of-input and would *stop the application* — pressing CapsLock would
//!   quit the app. The only exits are a modeled event or an `Err`.
//!
//! ## Why blocking-mode end-of-input is not a real concern
//!
//! `poll_event(None) == Ok(None)` is the runtime's clean-stop signal, and it
//! stays exercised by [`TestEventSource`](rstui_core::TestEventSource) (a
//! drained script) — but it is effectively *unreachable* for this live source,
//! by design. Verified against crossterm 0.29's source: its blocking
//! `event::read()` does not surface terminal EOF — on a closed tty the unix
//! reader breaks its inner read loop on a zero-length read while the outer poll
//! loop has no deadline, so it busy-spins rather than returning `Ok`/`Err`. A
//! real terminal does not reach EOF anyway: closing it delivers `SIGHUP` to the
//! process. So the contract is honored (a scripted source ends; an app quits
//! via `Cmd::quit`) while the live source's blocking loop legitimately only
//! ever yields a modeled event or an error.
//!
//! # Testability: the one PTY-only surface is two crossterm calls
//!
//! Mirroring [`CrosstermBackend`](crate::CrosstermBackend) being generic over
//! [`std::io::Write`], the source is generic over a private `RawEventReader`
//! seam. The real reader ([`CrosstermReader`]) is the *only* part that touches
//! the terminal device — its two `crossterm::event::{poll, read}` calls are the
//! sole ADR 0001 testing-layer L4c (PTY) surface. Every branch of the decision
//! logic (timeout vs. block, modeled vs. skipped, error propagation, and the
//! "CapsLock does not quit the app" property) is asserted in memory with a
//! scripted reader and **no terminal**, keeping the deterministic test story
//! intact for the one non-deterministic crate.

use std::io;
use std::time::Duration;

use rstui_core::event::Event;
use rstui_core::event_source::EventSource;

use crate::event::from_crossterm;

/// crossterm's `poll`/`read` pair, abstracted so the source's decision logic is
/// unit-testable without a TTY.
///
/// The input-side analog of [`CrosstermBackend`](crate::CrosstermBackend) being
/// generic over [`io::Write`]: the real reader ([`CrosstermReader`]) touches
/// the terminal device, while tests substitute a scripted reader to exercise
/// every branch of [`CrosstermEventSource::poll_event`] in memory. Kept private
/// — the public surface is just [`CrosstermEventSource::new`].
trait RawEventReader {
    /// Blocks until a native event is available (`crossterm::event::read`).
    fn read(&mut self) -> io::Result<crossterm::event::Event>;

    /// Waits up to `timeout` for availability (`crossterm::event::poll`); a
    /// `true` return guarantees the next [`read`](RawEventReader::read) will not
    /// block.
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
}

/// The production `RawEventReader`: crossterm's global terminal input.
///
/// Zero-sized; it forwards to `crossterm::event::poll`/`read`, which read the
/// process-global terminal device. This is the sole genuinely TTY-bound surface
/// of the event source (ADR 0001 testing layer L4c); everything else is
/// asserted in memory. It is the default type parameter of
/// [`CrosstermEventSource`], so applications never name it.
#[derive(Debug, Default, Clone, Copy)]
pub struct CrosstermReader;

impl RawEventReader for CrosstermReader {
    fn read(&mut self) -> io::Result<crossterm::event::Event> {
        crossterm::event::read()
    }

    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        crossterm::event::poll(timeout)
    }
}

/// An [`EventSource`] reading a crossterm
/// terminal and translating input into rstui's [`Event`] vocabulary.
///
/// Construct it with [`new`](CrosstermEventSource::new) and hand it (with a
/// [`CrosstermBackend`](crate::CrosstermBackend)) to `rstui_runtime::run`; the
/// identical `run` call takes a
/// [`TestEventSource`](rstui_core::TestEventSource) in tests. See the
/// [module docs](self) for the two poll modes and why the blocking path skips
/// unmodeled input rather than ending the loop.
#[derive(Debug)]
pub struct CrosstermEventSource<R = CrosstermReader> {
    reader: R,
}

impl CrosstermEventSource {
    /// A source reading the real crossterm terminal.
    ///
    /// The only public constructor: the framework is zero-config here. The
    /// internal reader seam exists purely for the in-memory tests.
    #[must_use]
    pub fn new() -> Self {
        Self {
            reader: CrosstermReader,
        }
    }
}

/// Concrete (not `#[derive]`d) so `CrosstermEventSource::default()` resolves the
/// reader to [`CrosstermReader`] with no annotation — a derived
/// `impl<R: Default>` cannot, since a type-parameter *default* does not drive
/// inference. Equivalent to [`new`](CrosstermEventSource::new).
impl Default for CrosstermEventSource {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> CrosstermEventSource<R> {
    /// Builds a source over an arbitrary raw reader — the in-memory test seam.
    ///
    /// Crate-internal on purpose: the public surface stays
    /// [`new`](CrosstermEventSource::new), and the only external reader is the
    /// real terminal one.
    #[cfg(test)]
    fn with_reader(reader: R) -> Self {
        Self { reader }
    }
}

impl<R: RawEventReader> EventSource for CrosstermEventSource<R> {
    type Error = io::Error;

    fn poll_event(&mut self, timeout: Option<Duration>) -> Result<Option<Event>, Self::Error> {
        match timeout {
            // Bounded wait: one poll, then at most one read. Deliberately does
            // not loop — after the single poll-guaranteed event a further read
            // could block past the deadline. An unmodeled event maps to
            // `Ok(None)`, which a timed caller reads as "nothing this tick".
            Some(timeout) => {
                if self.reader.poll(timeout)? {
                    Ok(from_crossterm(self.reader.read()?))
                } else {
                    Ok(None)
                }
            }
            // Unbounded wait: block until a *modeled* event. Unmodeled input
            // (e.g. a Kitty-only CapsLock) is skipped and the loop reads again.
            // Returning `Ok(None)` here would make the runtime stop the app on
            // such a key, so the only exits are a modeled event or an `Err`
            // (see the module docs on why EOF is not reachable here).
            None => loop {
                if let Some(event) = from_crossterm(self.reader.read()?) {
                    return Ok(Some(event));
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crossterm::event::{
        Event as CtEvent, KeyCode as CtKeyCode, KeyEvent as CtKeyEvent,
        KeyModifiers as CtKeyModifiers,
    };
    use rstui_core::event::KeyCode;
    use rstui_core::geometry::Size;

    use super::*;

    /// A scripted [`RawEventReader`]: `poll`/`read` pop their next queued
    /// outcome, so every branch of `poll_event` is driven deterministically
    /// with no terminal. `reads_made` proves the timed path issues at most one
    /// `read` and never reads after a `false` poll.
    #[derive(Default)]
    struct ScriptedReader {
        polls: VecDeque<io::Result<bool>>,
        reads: VecDeque<io::Result<CtEvent>>,
        reads_made: usize,
    }

    impl ScriptedReader {
        fn with_reads<I>(reads: I) -> Self
        where
            I: IntoIterator<Item = io::Result<CtEvent>>,
        {
            Self {
                reads: reads.into_iter().collect(),
                ..Self::default()
            }
        }

        fn push_poll(mut self, outcome: io::Result<bool>) -> Self {
            self.polls.push_back(outcome);
            self
        }
    }

    impl RawEventReader for ScriptedReader {
        fn read(&mut self) -> io::Result<CtEvent> {
            self.reads_made += 1;
            self.reads
                .pop_front()
                .expect("test script under-supplied reads")
        }

        fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
            self.polls
                .pop_front()
                .expect("test script under-supplied polls")
        }
    }

    fn source(reader: ScriptedReader) -> CrosstermEventSource<ScriptedReader> {
        CrosstermEventSource::with_reader(reader)
    }

    fn ct_char(c: char) -> CtEvent {
        CtEvent::Key(CtKeyEvent::new(CtKeyCode::Char(c), CtKeyModifiers::NONE))
    }

    /// A native event rstui deliberately does not model — `from_crossterm`
    /// drops it to `None`. Pressing it must not quit the app.
    fn ct_unmodeled() -> CtEvent {
        CtEvent::Key(CtKeyEvent::new(CtKeyCode::CapsLock, CtKeyModifiers::NONE))
    }

    #[test]
    fn timed_poll_timeout_yields_none_and_never_reads() {
        let mut src = source(ScriptedReader::default().push_poll(Ok(false)));

        assert_eq!(
            src.poll_event(Some(Duration::from_millis(5))).unwrap(),
            None
        );
        // A `false` poll must short-circuit before any read.
        assert_eq!(src.reader.reads_made, 0);
    }

    #[test]
    fn timed_poll_ready_returns_the_translated_event() {
        let mut src = source(ScriptedReader::with_reads([Ok(ct_char('k'))]).push_poll(Ok(true)));

        let event = src
            .poll_event(Some(Duration::ZERO))
            .unwrap()
            .expect("an event was ready");
        assert_eq!(event.as_key_press().unwrap().code, KeyCode::Char('k'));
        assert_eq!(src.reader.reads_made, 1);
    }

    #[test]
    fn timed_unmodeled_event_is_none_and_does_not_loop() {
        // Only ONE read is scripted: if the timed path looped trying to find a
        // modeled event it would panic on the under-supplied script. It must
        // instead report `Ok(None)` ("nothing this tick") after one read.
        let mut src = source(ScriptedReader::with_reads([Ok(ct_unmodeled())]).push_poll(Ok(true)));

        assert_eq!(src.poll_event(Some(Duration::ZERO)).unwrap(), None);
        assert_eq!(src.reader.reads_made, 1);
    }

    #[test]
    fn timed_poll_error_propagates() {
        let mut src =
            source(ScriptedReader::default().push_poll(Err(io::Error::other("poll failed"))));

        let err = src.poll_event(Some(Duration::ZERO)).unwrap_err();
        assert_eq!(err.to_string(), "poll failed");
        assert_eq!(src.reader.reads_made, 0);
    }

    #[test]
    fn blocking_skips_unmodeled_input_then_returns_the_modeled_event() {
        // The "CapsLock does not quit the app" property: two unmodeled events
        // are skipped and the loop blocks on, then returns, the modeled one —
        // it must not surface `Ok(None)` (which the runtime treats as stop).
        let mut src = source(ScriptedReader::with_reads([
            Ok(ct_unmodeled()),
            Ok(ct_unmodeled()),
            Ok(ct_char('a')),
        ]));

        let event = src.poll_event(None).unwrap().expect("a modeled event");
        assert_eq!(event.as_key_press().unwrap().code, KeyCode::Char('a'));
        assert_eq!(src.reader.reads_made, 3);
    }

    #[test]
    fn blocking_returns_a_modeled_event_immediately() {
        let mut src = source(ScriptedReader::with_reads([Ok(ct_char('z'))]));

        let event = src.poll_event(None).unwrap().expect("a modeled event");
        assert_eq!(event.as_key_press().unwrap().code, KeyCode::Char('z'));
        assert_eq!(src.reader.reads_made, 1);
    }

    #[test]
    fn blocking_read_error_propagates_rather_than_ending_input() {
        // An `Err` from `read` is a real failure: it must surface as `Err`
        // (`RunError::Input`), never be swallowed into `Ok(None)`.
        let mut src = source(ScriptedReader::with_reads([Err(io::Error::other(
            "device gone",
        ))]));

        let err = src.poll_event(None).unwrap_err();
        assert_eq!(err.to_string(), "device gone");
    }

    #[test]
    fn non_key_events_pass_through_in_blocking_mode() {
        // Proves the source is not key-only: a resize round-trips through
        // `from_crossterm` like any other event.
        let mut src = source(ScriptedReader::with_reads([Ok(CtEvent::Resize(80, 24))]));

        assert_eq!(
            src.poll_event(None).unwrap(),
            Some(Event::Resize(Size::new(80, 24))),
        );
    }

    /// The public surface is exactly `new()`/`Default`, and the resulting type
    /// is a real [`EventSource`] (so `rstui_runtime::run` accepts it). Compiles
    /// only if `CrosstermEventSource<CrosstermReader>: EventSource`.
    #[test]
    fn public_constructors_build_a_usable_event_source() {
        fn assert_event_source<S: EventSource>(_: &S) {}

        let from_new = CrosstermEventSource::new();
        let from_default = CrosstermEventSource::default();
        assert_event_source(&from_new);
        assert_event_source(&from_default);
    }
}
