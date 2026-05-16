//! The crossterm [`AsyncEventSource`]: `EventStream`-driven, unmodeled input
//! skipped — the async dual of [`CrosstermEventSource`](crate::CrosstermEventSource).
//!
//! Available only with the `async` cargo feature (ADR 0011). It wraps
//! crossterm's runtime-agnostic [`EventStream`](crossterm::event::EventStream)
//! (a `futures::Stream` — crossterm pulls no tokio of its own) and translates
//! each native event through the already-tested [`from_crossterm`] map, so the
//! deterministic event vocabulary is unchanged: this is the *only* extra
//! surface async adds on the input side, and even it is unit-tested in memory
//! with a scripted stream (no TTY).
//!
//! It implements [`rstui_runtime::AsyncEventSource`], so the same
//! `rstui_runtime::run_async` `tokio::select!` loop the headless async tests
//! drive runs an unmodified [`App`](https://docs.rs/rstui-runtime) live over a
//! [`TerminalGuard`](crate::TerminalGuard) + this source — see
//! [`run_app_async`](crate::run_app_async).
//!
//! # Unmodeled input is skipped, not end-of-stream
//!
//! Like the sync source's *blocking* mode, an event rstui deliberately does
//! not model (a Kitty-only `CapsLock`, …) maps to `None` from
//! [`from_crossterm`] and is **skipped** — the loop reads the next stream item
//! rather than reporting it. `Ok(None)` is reserved for its single, correct
//! meaning: the stream **ended** (terminal closed). Returning `Ok(None)` for
//! an unmodeled key would make `run_async` treat a CapsLock press as
//! end-of-input and quit the app — the same bug the sync source documents.
//!
//! # Cancel-safety
//!
//! [`AsyncEventSource::next_event`] must be cancel-safe (a `tokio::select!`
//! drops the losing branch's future). crossterm's `EventStream` reads on an
//! internal thread into a buffered channel and yields from it, so dropping a
//! partially-driven `next()` does **not** lose an event — it stays buffered
//! for the next poll. This source therefore satisfies the contract
//! `rstui_runtime::run_async` requires.
//!
//! # Testability: the one PTY-only surface is the real `EventStream`
//!
//! Mirroring [`CrosstermEventSource`](crate::CrosstermEventSource) being
//! generic over a private `RawEventReader`, this is generic over a private
//! `AsyncEventReader` stream seam. The real reader
//! ([`CrosstermEventStream`]) is the only part that touches the terminal
//! device (ADR 0001 testing layer L4c); every decision branch — translate,
//! skip-unmodeled, error, end-of-stream — is asserted in memory against a
//! scripted reader with no terminal and no async runtime.

#[cfg(test)]
use std::collections::VecDeque;
use std::io;

use rstui_core::event::Event;
use rstui_runtime::AsyncEventSource;

use crate::event::from_crossterm;

/// crossterm's input `Stream`, abstracted so the source's decision logic is
/// unit-testable without a TTY (the async analog of the sync source's
/// `RawEventReader`).
///
/// One method: await the next native item — `Some(Ok(event))`,
/// `Some(Err(io))`, or `None` (stream ended). Kept private; the public
/// surface is just [`CrosstermAsyncEventSource::new`].
trait AsyncEventReader {
    /// Awaits the next native event from the underlying stream.
    fn next(
        &mut self,
    ) -> impl std::future::Future<Output = Option<io::Result<crossterm::event::Event>>> + Send;
}

/// The production `AsyncEventReader`: crossterm's
/// [`EventStream`](crossterm::event::EventStream).
///
/// The sole genuinely TTY-bound surface of the async source (ADR 0001 testing
/// layer L4c); it is the default type parameter of
/// [`CrosstermAsyncEventSource`], so applications never name it.
#[derive(Debug, Default)]
pub struct CrosstermEventStream {
    inner: crossterm::event::EventStream,
}

impl AsyncEventReader for CrosstermEventStream {
    fn next(
        &mut self,
    ) -> impl std::future::Future<Output = Option<io::Result<crossterm::event::Event>>> + Send {
        // `StreamExt::next` borrows the stream and yields its next item; the
        // returned future is `Send` (crossterm's `EventStream` is `Send`).
        futures_util::StreamExt::next(&mut self.inner)
    }
}

/// An [`AsyncEventSource`] reading a crossterm terminal and translating input
/// into rstui's [`Event`] vocabulary.
///
/// Construct it with [`new`](CrosstermAsyncEventSource::new) and hand it (with
/// a [`CrosstermBackend`](crate::CrosstermBackend) in a
/// [`TerminalGuard`](crate::TerminalGuard)) to `rstui_runtime::run_async`;
/// [`run_app_async`](crate::run_app_async) composes exactly that for you.
#[derive(Debug)]
pub struct CrosstermAsyncEventSource<R = CrosstermEventStream> {
    reader: R,
}

impl CrosstermAsyncEventSource {
    /// A source reading the real crossterm terminal asynchronously.
    ///
    /// The only public constructor: the framework is zero-config here. The
    /// internal reader seam exists purely for the in-memory tests.
    #[must_use]
    pub fn new() -> Self {
        Self {
            reader: CrosstermEventStream::default(),
        }
    }
}

/// Concrete (not `#[derive]`d) so `CrosstermAsyncEventSource::default()`
/// resolves the reader to [`CrosstermEventStream`] with no annotation —
/// equivalent to [`new`](CrosstermAsyncEventSource::new).
impl Default for CrosstermAsyncEventSource {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> CrosstermAsyncEventSource<R> {
    /// Builds a source over an arbitrary async reader — the in-memory test
    /// seam. Crate-internal: the public surface stays
    /// [`new`](CrosstermAsyncEventSource::new).
    #[cfg(test)]
    fn with_reader(reader: R) -> Self {
        Self { reader }
    }
}

impl<R: AsyncEventReader + Send> AsyncEventSource for CrosstermAsyncEventSource<R> {
    type Error = io::Error;

    async fn next_event(&mut self) -> Result<Option<Event>, io::Error> {
        loop {
            match self.reader.next().await {
                // A modeled event: translate and deliver.
                Some(Ok(native)) => {
                    if let Some(event) = from_crossterm(native) {
                        return Ok(Some(event));
                    }
                    // Unmodeled (Kitty-only CapsLock, …): skip and read on —
                    // *not* end-of-input (see the module docs).
                }
                Some(Err(error)) => return Err(error),
                // Stream ended for good: the one true meaning of `Ok(None)`.
                None => return Ok(None),
            }
        }
    }
}

/// A scripted `AsyncEventReader` for the in-memory tests: each `next` pops
/// the next queued item. No TTY, no async runtime — every item is immediately
/// ready, so the decision logic is driven deterministically.
#[cfg(test)]
#[derive(Default)]
struct ScriptedReader {
    items: VecDeque<io::Result<crossterm::event::Event>>,
}

#[cfg(test)]
impl ScriptedReader {
    fn with<I>(items: I) -> Self
    where
        I: IntoIterator<Item = io::Result<crossterm::event::Event>>,
    {
        Self {
            items: items.into_iter().collect(),
        }
    }
}

#[cfg(test)]
impl AsyncEventReader for ScriptedReader {
    fn next(
        &mut self,
    ) -> impl std::future::Future<Output = Option<io::Result<crossterm::event::Event>>> + Send {
        let item = self.items.pop_front();
        async move { item }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use crossterm::event::{
        Event as CtEvent, KeyCode as CtKeyCode, KeyEvent as CtKeyEvent,
        KeyModifiers as CtKeyModifiers,
    };
    use rstui_core::event::KeyCode;
    use rstui_core::geometry::Size;

    use super::*;

    /// Drives a future to completion with the stable no-op [`Waker`]. Sound
    /// here because the scripted reader's futures are always immediately
    /// `Ready`, so the loop converges in one poll — no runtime, no real
    /// waker, dependency-free and deterministic (the `unsafe`-free `unsafe`-
    /// forbidden workspace cannot hand-roll a `RawWaker`, and need not).
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
        }
    }

    fn ct_char(c: char) -> CtEvent {
        CtEvent::Key(CtKeyEvent::new(CtKeyCode::Char(c), CtKeyModifiers::NONE))
    }

    /// A native event rstui does not model — `from_crossterm` drops it; it
    /// must be skipped, never reported as end-of-stream.
    fn ct_unmodeled() -> CtEvent {
        CtEvent::Key(CtKeyEvent::new(CtKeyCode::CapsLock, CtKeyModifiers::NONE))
    }

    #[test]
    fn translates_a_modeled_event() {
        let mut src =
            CrosstermAsyncEventSource::with_reader(ScriptedReader::with([Ok(ct_char('k'))]));
        let event = block_on(src.next_event()).unwrap().expect("an event");
        assert_eq!(event.as_key_press().unwrap().code, KeyCode::Char('k'));
    }

    #[test]
    fn skips_unmodeled_input_then_returns_the_next_modeled_event() {
        // Two unmodeled events are skipped and the modeled one delivered — it
        // must NOT surface `Ok(None)` (which `run_async` treats as stop).
        let mut src = CrosstermAsyncEventSource::with_reader(ScriptedReader::with([
            Ok(ct_unmodeled()),
            Ok(ct_unmodeled()),
            Ok(ct_char('a')),
        ]));
        let event = block_on(src.next_event())
            .unwrap()
            .expect("a modeled event");
        assert_eq!(event.as_key_press().unwrap().code, KeyCode::Char('a'));
    }

    #[test]
    fn a_drained_stream_is_end_of_input() {
        let mut src = CrosstermAsyncEventSource::with_reader(ScriptedReader::default());
        assert_eq!(block_on(src.next_event()).unwrap(), None);
    }

    #[test]
    fn a_read_error_propagates_rather_than_ending_input() {
        let mut src = CrosstermAsyncEventSource::with_reader(ScriptedReader::with([Err(
            io::Error::other("device gone"),
        )]));
        let err = block_on(src.next_event()).unwrap_err();
        assert_eq!(err.to_string(), "device gone");
    }

    #[test]
    fn non_key_events_pass_through() {
        let mut src = CrosstermAsyncEventSource::with_reader(ScriptedReader::with([Ok(
            CtEvent::Resize(80, 24),
        )]));
        assert_eq!(
            block_on(src.next_event()).unwrap(),
            Some(Event::Resize(Size::new(80, 24))),
        );
    }

    /// The type is a real [`AsyncEventSource`], so `rstui_runtime::run_async`
    /// accepts it. Asserted through the scripted seam: the public `new()` /
    /// `Default` construct a real `crossterm::event::EventStream`, which
    /// initializes the process-global terminal reader and so is the L4c
    /// PTY-only surface (constructing it with no TTY panics — the same reason
    /// the sync source documents its real reader as untestable). The trait
    /// bound is what matters, and it is proven here with no terminal.
    #[test]
    fn the_scripted_seam_is_a_usable_async_event_source() {
        fn assert_async_source<S: AsyncEventSource>(_: &S) {}
        assert_async_source(&CrosstermAsyncEventSource::with_reader(
            ScriptedReader::default(),
        ));
    }
}
