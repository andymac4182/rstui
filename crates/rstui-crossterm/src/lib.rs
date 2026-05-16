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
//! crossterm input source. This slice delivers the first of them: a pure,
//! terminal-free translation from crossterm's native input into rstui's owned
//! event model.
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

pub mod event;

pub use event::from_crossterm;
