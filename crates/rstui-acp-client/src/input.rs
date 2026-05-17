//! Async terminal input: a [`rstui_runtime::AsyncEventSource`] over crossterm's
//! `EventStream`.
//!
//! `rstui-crossterm` ships only the *sync* `CrosstermEventSource` (its async
//! `EventStream` source is recorded there as "a future opt"). This client needs
//! the async loop (ADR 0011) because ACP is inherently streaming, so it bridges
//! crossterm's `event-stream` feature into rstui here, reusing
//! [`rstui_crossterm::from_crossterm`] for the *exact same* total, terminal-free
//! crossterm→rstui translation the sync source uses — no divergent input model.

use std::io;

use crossterm::event::EventStream;
use futures::StreamExt;
use rstui_runtime::{AsyncEventSource, Event};

/// Streams translated terminal events for the async runtime loop.
///
/// Unmodeled crossterm input (Kitty-only lock/media keys) is skipped rather
/// than reported as end-of-input, matching `CrosstermEventSource`'s contract:
/// `Ok(None)` is *only* a permanently closed input stream.
pub struct TerminalEvents {
    stream: EventStream,
}

impl TerminalEvents {
    /// Opens the crossterm event stream.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stream: EventStream::new(),
        }
    }
}

impl Default for TerminalEvents {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncEventSource for TerminalEvents {
    type Error = io::Error;

    // The trait spells the return type `-> impl Future + Send` precisely
    // because `async fn` in a trait cannot express the `Send` bound the
    // multi-threaded `tokio::select!` loop requires (see `AsyncEventSource`
    // docs). `manual_async_fn` would have us drop exactly that.
    #[allow(clippy::manual_async_fn)]
    fn next_event(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<Event>, io::Error>> + Send {
        async move {
            loop {
                // `EventStream::next()` is cancel-safe (a lost `select!` branch
                // drops a pending read without losing a buffered event), which
                // is exactly the `AsyncEventSource` contract.
                match self.stream.next().await {
                    None => return Ok(None),
                    Some(Err(err)) => return Err(err),
                    Some(Ok(native)) => {
                        if let Some(event) = rstui_crossterm::from_crossterm(native) {
                            return Ok(Some(event));
                        }
                        // Unmodeled input: keep waiting, never stop the app.
                    }
                }
            }
        }
    }
}
