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
//!   draw into and renderers diff, including [`Buffer::clear_region`] — the
//!   opaque-overlay primitive a floating widget (modal, popup) reclaims its
//!   area through, since a style patch alone cannot.
//! - [`backend`]: the [`Backend`] screen boundary plus an in-memory
//!   [`TestBackend`] so every layer above can be tested without a TTY.
//! - [`terminal`]: the [`Terminal`] frame driver that runs the
//!   draw → diff → flush → swap loop a [`Frame`] at a time.
//! - [`event`]: the keyboard/mouse/focus/resize [`Event`] vocabulary the
//!   runtime, components, and focus routing all share.
//! - [`event_source`]: the [`EventSource`] input boundary (the dual of
//!   [`Backend`]) plus an in-memory [`TestEventSource`] so whole apps can be
//!   driven by a scripted event stream without a TTY, and a
//!   [`ChannelEventSource`] another thread feeds over `std::sync::mpsc` —
//!   a second production-shaped source proving the boundary is not
//!   crossterm-only.
//! - [`focus`]: the optional, caller-owned focus model — [`FocusId`] value
//!   tokens and a pure, total [`FocusRing`] (with a model-owned modal
//!   focus-scope stack: `push_scope`/`pop_scope`, validated capture/restore,
//!   declarative reducer-gated trapping) the app stores and `view` reads,
//!   never runtime- or widget-owned ([ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)).
//! - [`widget`]: the [`Widget`] rendering abstraction every component
//!   implements. Concrete widgets (`Block`, `Paragraph`, …) live in the
//!   separate `rstui-widgets` crate so this crate stays primitives-only
//!   ([ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)).
//! - [`text`]: the styled-text model ([`Span`], [`Line`], [`Text`]) every
//!   richer component composes, with a predictable text→line→span style
//!   cascade.
//! - [`text_edit`]: the optional, caller-owned single-line editing model —
//!   [`TextEdit`], a pure, total `String`+character-cursor value the app
//!   stores and `update` mutates, the editing-side dual of [`FocusRing`]
//!   that an `Input` widget projects ([ADR 0004](https://github.com/andymac4182/rstui/blob/main/docs/adr/0004-focus-routing-architecture.md)).
//! - [`text_area`]: the optional, caller-owned **multi-line** editing model —
//!   [`TextArea`], the document dual of [`TextEdit`] (a `Vec<String>` of
//!   logical lines plus a `(row, col)` char-indexed cursor with a sticky goal
//!   column); a pure, total value the app stores and `update` mutates, that
//!   an `Editor` widget projects (ADR 0004 Follow-up §2).
//! - [`scroll`]: the optional, caller-owned scroll/viewport model —
//!   [`ScrollState`], a pure, total `offset` + `follow_tail` value the app
//!   stores and `update` mutates (clamp, scroll-by, sticky-bottom-while-
//!   streaming, scroll-into-view) that a `ScrollView` projects
//!   ([ADR 0012](https://github.com/andymac4182/rstui/blob/main/docs/adr/0012-widget-composition-and-layout-model.md) §P0).
//! - [`selection`]: the optional, caller-owned text-selection model —
//!   [`Selection`] (a row-major terminal-stream span over content
//!   coordinates) plus [`selected_text`], a pure, total projection the app
//!   stores and `update` mutates on drag; widgets read `contains` and the
//!   app extracts the copied text (ADR 0012 §P1).
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
pub mod focus;
pub mod geometry;
pub mod layout;
pub mod scroll;
pub mod selection;
pub mod style;
pub mod stylize;
pub mod terminal;
pub mod text;
pub mod text_area;
pub mod text_edit;
pub mod widget;

pub use backend::{Backend, TestBackend};
pub use buffer::{Buffer, Cell};
pub use event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub use event_source::{ChannelEventSource, EventSource, TestEventSource};
pub use focus::{FocusId, FocusRing};
pub use geometry::{Margin, Position, Rect, Size};
pub use layout::{Alignment, Constraint, Direction, Layout};
pub use scroll::ScrollState;
pub use selection::{Selection, selected_text};
pub use style::{Color, ColorLevel, Modifier, Style};
pub use stylize::{Styled, Stylize};
pub use terminal::{CompletedFrame, Frame, Terminal};
pub use text::{Line, Span, Text};
pub use text_area::TextArea;
pub use text_edit::TextEdit;
pub use widget::Widget;
