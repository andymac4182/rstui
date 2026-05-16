//! `rstui-core` — the foundational rendering substrate for the rstui TUI
//! framework.
//!
//! This crate intentionally has no dependencies and knows nothing about
//! terminals, async runtimes, or the application event loop. It provides the
//! pure, deterministic primitives every higher layer builds on:
//!
//! - [`geometry`]: integer screen coordinates ([`Position`], [`Size`],
//!   [`Rect`], [`Margin`]).
//! - [`style`]: composable colors and attributes ([`Color`], [`Modifier`],
//!   [`Style`]).
//! - [`buffer`]: the immediate-mode [`Cell`] grid ([`Buffer`]) that widgets
//!   draw into and renderers diff.
//! - [`backend`]: the [`Backend`] screen boundary plus an in-memory
//!   [`TestBackend`] so every layer above can be tested without a TTY.
//!
//! Keeping these pieces dependency-free and panic-light makes them trivial to
//! unit test without a real terminal, which is the property the rest of the
//! framework (runtime, components, plugin host) will lean on.
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Color, Modifier, Position, Rect, Style};
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 12, 1));
//! buf.set_str(
//!     Position::ORIGIN,
//!     "hello rstui",
//!     Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
//! );
//!
//! let cell = buf.get(Position::ORIGIN).unwrap();
//! assert_eq!(cell.symbol, 'h');
//! assert_eq!(cell.fg, Color::Green);
//! assert!(cell.modifier.contains(Modifier::BOLD));
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod backend;
pub mod buffer;
pub mod geometry;
pub mod style;

pub use backend::{Backend, TestBackend};
pub use buffer::{Buffer, Cell};
pub use geometry::{Margin, Position, Rect, Size};
pub use style::{Color, Modifier, Style};
