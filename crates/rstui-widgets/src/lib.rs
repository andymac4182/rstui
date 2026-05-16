//! `rstui-widgets` — the concrete widget set for the rstui TUI framework.
//!
//! `rstui-core` owns the [`Widget`](rstui_core::Widget) trait and the
//! dependency-free primitives (buffer, geometry, style, layout, the text
//! model). This crate is where the actual *widgets* live, one module per
//! widget, so the universally-depended-on core stays small and slow-moving
//! (see [ADR 0002](https://github.com/andymac4182/rstui/blob/main/docs/adr/0002-widget-crate-boundary.md)).
//!
//! Every widget here follows the exact pattern a third-party widget crate
//! follows — depend on `rstui-core`, implement
//! [`rstui_core::Widget`], stamp glyphs through the public
//! [`Buffer::set_cell`](rstui_core::Buffer::set_cell) /
//! [`Buffer::set_str`](rstui_core::Buffer::set_str) /
//! [`Buffer::set_style`](rstui_core::Buffer::set_style) contract, and
//! snapshot-test against
//! [`TestBackend`](rstui_core::TestBackend) — so this crate doubles as the
//! worked reference for building your own.
//!
//! - [`block`]: [`Block`] — the foundational container (borders, a styled
//!   fill, padding, a clipped [`Line`](rstui_core::Line) title), plus
//!   [`Borders`], [`BorderType`], [`BorderSet`], and [`Padding`].
//! - [`paragraph`]: [`Paragraph`] — the multi-line text widget with soft word
//!   [`Wrap`], scroll, alignment, and an optional framing [`Block`].
//!
//! # Example
//!
//! ```
//! use rstui_core::{Buffer, Position, Rect, Widget};
//! use rstui_widgets::{Block, Borders};
//!
//! let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
//! Block::bordered().title("Hi").render(buf.area(), &mut buf);
//!
//! assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
//! assert_eq!(buf.get(Position::new(1, 0)).unwrap().symbol, 'H');
//! assert_eq!(Block::bordered().inner(buf.area()), Rect::new(1, 1, 4, 1));
//! ```

pub mod block;
pub mod paragraph;

pub use block::{Block, BorderSet, BorderType, Borders, Padding};
pub use paragraph::{Paragraph, Wrap};
