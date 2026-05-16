//! `rstui-crossterm` — the crossterm-backed terminal driver for rstui.
//!
//! Per [ADR 0001](https://github.com/andymac4182/rstui/blob/main/docs/adr/0001-terminal-backend-strategy.md)
//! this crate is the single home of the workspace's only external dependency:
//! it owns crossterm so `rstui-core` can stay dependency-free. Application code
//! depends on `rstui-core`'s [`Backend`](rstui_core::backend::Backend) trait and
//! [`Event`](rstui_core::event::Event) vocabulary, never on crossterm directly,
//! so the backend can be swapped without touching apps.
//!
//! The crate's eventual responsibilities are the `Backend` implementation over
//! an [`std::io::Write`], a panic-safe RAII terminal-lifecycle guard, and the
//! crossterm input source. Two of them have landed: the pure, terminal-free
//! input translation ([`from_crossterm`]) and the
//! [`CrosstermBackend`] drawing seam. The panic-safe lifecycle guard is the
//! remaining slice.
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

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod backend;
pub mod event;

pub use backend::CrosstermBackend;
pub use event::from_crossterm;
