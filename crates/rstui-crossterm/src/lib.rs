//! `rstui-crossterm` — the crossterm-backed terminal driver for rstui.
//!
//! Per [ADR 0001](https://github.com/andymac4182/rstui/blob/main/docs/adr/0001-terminal-backend-strategy.md)
//! this crate is the single home of the workspace's only external dependency:
//! it owns crossterm so `rstui-core` can stay dependency-free. Application code
//! depends on `rstui-core`'s [`Backend`](rstui_core::backend::Backend) trait and
//! [`Event`](rstui_core::event::Event) vocabulary, never on crossterm directly,
//! so the backend can be swapped without touching apps.
//!
//! The crate's responsibilities are the `Backend` implementation over an
//! [`std::io::Write`], a panic-safe RAII terminal-lifecycle guard, the pure
//! crossterm→rstui input translation, the crossterm input source, and the
//! one-call full-screen app shell. **All have landed**: the terminal-free
//! input translation ([`from_crossterm`]), the [`CrosstermBackend`] drawing
//! seam, the [`TerminalGuard`] panic-safe lifecycle guard, the
//! [`CrosstermEventSource`] input source, and [`run_app`] — the ergonomic
//! entry point that composes all four with [`rstui_runtime::run`] and a
//! panic-restore hook. The framework now composes end to end: the same
//! `rstui_runtime::run` the headless harness tests drive runs an unmodified
//! app on a real terminal in a single call (see the `run_app` example). A
//! feature-gated async `EventStream` source is a future enhancement.
//!
//! # App shell: one call from `App` to a live terminal
//!
//! [`run_app`] hides the four-seam composition every full-screen `main` would
//! otherwise repeat, and installs a panic hook that restores the terminal
//! *before* the panic message prints, so a crash leaves the user's shell clean
//! **and** readable. See the [`shell`] module for the panic policy.
//!
//! ```no_run
//! use rstui_crossterm::run_app;
//! # use rstui_runtime::{App, Cmd, Frame};
//! # #[derive(Default)] struct Editor;
//! # impl App for Editor {
//! #     type Message = ();
//! #     fn update(&mut self, _: ()) -> Cmd<()> { Cmd::quit() }
//! #     fn view(&self, _: &mut Frame<'_>) {}
//! # }
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     run_app(Editor::default())?;
//!     Ok(())
//! }
//! ```
//!
//! # Output: the backend
//!
//! [`CrosstermBackend`] implements `rstui-core`'s
//! [`Backend`](rstui_core::backend::Backend) over any [`std::io::Write`]. Every
//! escape sequence it emits is queued (never `execute!`d) and asserted in
//! memory with no TTY (ADR 0001 testing layer L4b); only `size`/
//! `cursor_position` query the real terminal. See the
//! [`backend`] module for the full rationale.
//!
//! ```
//! use rstui_core::backend::Backend;
//! use rstui_core::buffer::Cell;
//! use rstui_core::geometry::Position;
//! use rstui_core::style::Color;
//! use rstui_crossterm::CrosstermBackend;
//!
//! // A real backend wraps `std::io::stdout()`; here, an in-memory buffer so
//! // the emitted ANSI is assertable without a terminal.
//! let mut backend = CrosstermBackend::new(Vec::new());
//!
//! let mut cell = Cell::new('R');
//! cell.fg = Color::Red;
//! backend.draw([(Position::new(1, 0), &cell)]).unwrap();
//! backend.flush().unwrap();
//!
//! assert!(!backend.writer().is_empty());
//! ```
//!
//! # Event translation
//!
//! [`from_crossterm`] maps a [`crossterm::event::Event`] to an
//! [`rstui_core::event::Event`]. Because rstui deliberately shaped its core
//! event vocabulary 1:1 like crossterm's (a recorded, intentional divergence
//! from ratatui, which re-exports crossterm's types), this map is
//! near-mechanical and — crucially — **unit-testable with hand-built events,
//! no terminal required**, which keeps the deterministic test story intact
//! even for the one non-deterministic crate.
//!
//! Codes rstui does not model (the Kitty-only lock/media/modifier keys) yield
//! [`None`] rather than a stubbed variant, matching rstui's "defer, do not
//! stub" discipline; callers `filter_map` the input stream.
//!
//! ```
//! use crossterm::event::{
//!     Event as CtEvent, KeyCode as CtKeyCode, KeyEvent as CtKeyEvent,
//!     KeyModifiers as CtKeyModifiers,
//! };
//! use rstui_core::event::{KeyCode, KeyModifiers};
//! use rstui_crossterm::from_crossterm;
//!
//! // A real backend reads these from the terminal; here, by hand.
//! let native = CtEvent::Key(CtKeyEvent::new(
//!     CtKeyCode::Char('c'),
//!     CtKeyModifiers::CONTROL,
//! ));
//!
//! let key = from_crossterm(native).unwrap().as_key_press().unwrap();
//! assert_eq!(key.code, KeyCode::Char('c'));
//! assert!(key.modifiers.contains(KeyModifiers::CONTROL));
//!
//! // A Kitty-only code rstui does not model is dropped, not stubbed.
//! assert!(from_crossterm(CtEvent::Key(CtKeyEvent::new(
//!     CtKeyCode::CapsLock,
//!     CtKeyModifiers::NONE,
//! )))
//! .is_none());
//! ```
//!
//! # Input: the event source
//!
//! [`CrosstermEventSource`] implements `rstui-core`'s
//! [`EventSource`](rstui_core::event_source::EventSource) — the input dual of
//! [`CrosstermBackend`]. It folds crossterm's `poll`/`read` into one timed call
//! and translates each native event through [`from_crossterm`]. The blocking
//! mode **skips** input rstui does not model (so a CapsLock press is ignored,
//! not read as end-of-input that would stop the app); the timed mode does one
//! poll and at most one read so an animation tick can never be starved. Only
//! the real reader's two `crossterm::event::{poll, read}` calls touch a TTY;
//! every decision branch is asserted in memory. See the [`event_source`] module
//! for the full rationale.
//!
//! ```no_run
//! use rstui_core::event_source::EventSource;
//! use rstui_crossterm::CrosstermEventSource;
//!
//! // Reads the real terminal (hence `no_run`). The same value, with a
//! // `CrosstermBackend`, is what `rstui_runtime::run` drives an app over.
//! let mut input = CrosstermEventSource::new();
//!
//! // Unbounded poll blocks until the next *modeled* event; unmodeled input
//! // (e.g. CapsLock) is skipped, never reported as end-of-input.
//! match input.poll_event(None)? {
//!     Some(event) => {
//!         let _ = event;
//!     }
//!     None => {} // a real terminal does not reach this (see module docs)
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! # Lifecycle: the panic-safe RAII guard
//!
//! [`TerminalGuard`] enables the requested terminal modes (raw mode, alternate
//! screen, mouse/paste/focus reporting — see [`LifecycleOptions`]) on
//! construction and restores exactly those on drop, **including while
//! unwinding from a panic**. It wraps a [`CrosstermBackend`] and is itself a
//! [`Backend`](rstui_core::backend::Backend), so it drops straight into
//! [`Terminal`](rstui_core::Terminal), giving one panic-safe ownership chain.
//! A deliberate divergence from ratatui (free `init`/`restore` + a manual
//! panic hook), affordable because rstui owns the loop. See the [`lifecycle`]
//! module for the proven ordering and the in-memory testability.
//!
//! ```
//! use rstui_crossterm::{CrosstermBackend, LifecycleOptions, TerminalGuard};
//!
//! // raw mode off + in-memory writer => no terminal required.
//! let backend = CrosstermBackend::new(Vec::new());
//! let opts = LifecycleOptions {
//!     raw_mode: false,
//!     ..LifecycleOptions::default()
//! };
//! let guard = TerminalGuard::with_options(backend, opts).unwrap();
//! assert!(!guard.backend().writer().is_empty()); // enter sequence sent
//! drop(guard); // matching disable sequence sent (here, and on panic unwind)
//! ```

pub mod backend;
pub mod event;
pub mod event_source;
pub mod lifecycle;
pub mod shell;

pub use backend::CrosstermBackend;
pub use event::from_crossterm;
pub use event_source::CrosstermEventSource;
pub use lifecycle::{LifecycleOptions, TerminalGuard};
pub use shell::{CrosstermRunError, restore_terminal, run_app, run_app_with};
