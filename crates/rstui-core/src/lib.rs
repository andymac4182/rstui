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
//! - [`stylize`]: the [`Stylize`] fluent shorthand trait (`"x".green().bold()`,
//!   `.on_blue()`) over any [`Styled`] value, including `&str`/[`Span`].
//! - [`layout`]: dividing a [`Rect`] into contiguous sub-regions with
//!   [`Constraint`]s ([`Layout`], [`Direction`]), and [`Alignment`] — the
//!   horizontal placement primitive the text model and widgets share.
//! - [`buffer`]: the immediate-mode [`Cell`] grid ([`Buffer`]) that widgets
//!   draw into and renderers diff.
//! - [`backend`]: the [`Backend`] screen boundary plus an in-memory
//!   [`TestBackend`] so every layer above can be tested without a TTY.
//! - [`terminal`]: the [`Terminal`] frame driver that runs the
//!   draw → diff → flush → swap loop a [`Frame`] at a time.
//! - [`event`]: the keyboard/mouse/focus/resize [`Event`] vocabulary the
//!   runtime, components, and focus routing all share.
//! - [`event_source`]: the [`EventSource`] input boundary (the dual of
//!   [`Backend`]) plus an in-memory [`TestEventSource`] so whole apps can be
//!   driven by a scripted event stream without a TTY.
//! - [`widget`]: the [`Widget`] rendering abstraction every component
//!   implements. Concrete widgets (`Block`, `Paragraph`, …) live in the
//!   separate `rstui-widgets` crate so this crate stays primitives-only
//!   ([ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)).
//! - [`text`]: the styled-text model ([`Span`], [`Line`], [`Text`]) every
//!   richer component composes, with a predictable text→line→span style
//!   cascade.
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

pub mod backend;
pub mod buffer;
pub mod event;
pub mod event_source;
pub mod geometry;
pub mod layout;
pub mod style;
pub mod stylize;
pub mod terminal;
pub mod text;
pub mod widget;

pub use backend::{Backend, TestBackend};
pub use buffer::{Buffer, Cell};
pub use event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub use event_source::{EventSource, TestEventSource};
pub use geometry::{Margin, Position, Rect, Size};
pub use layout::{Alignment, Constraint, Direction, Layout};
pub use style::{Color, Modifier, Style};
pub use stylize::{Styled, Stylize};
pub use terminal::{CompletedFrame, Frame, Terminal};
pub use text::{Line, Span, Text};
pub use widget::Widget;
