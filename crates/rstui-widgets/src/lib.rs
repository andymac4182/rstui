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
//! - [`list`]: [`List`] — a scrollable single-select column of [`ListItem`]
//!   rows with a highlight bar/gutter, rendered as a pure projection of
//!   caller-owned `selected`/`offset` state.
//! - [`tabs`]: [`Tabs`] — a one-row horizontal title strip with one selected,
//!   the same caller-owned pure projection as [`List`] on the other axis.
//! - [`gauge`]: [`Gauge`] — a horizontal progress bar, the first widget to
//!   render at sub-cell precision (fractional eighth-block glyphs).
//! - [`scrollbar`]: [`Scrollbar`] — a track-and-thumb scroll indicator, a
//!   pure projection of caller-owned scroll metrics; the visible companion to
//!   [`List`]/[`Paragraph`] scrolling and the first widget with no lifetime
//!   (every part is a single `char`).
//! - [`spinner`]: [`Spinner`] — a one-cell animated busy indicator, a pure
//!   projection of a caller-owned animation `tick`; the first consumer of the
//!   `Frame::count()` animation clock.
//! - [`table`]: [`Table`] — a column-aligned grid of [`Row`]s with an optional
//!   fixed header and single-row selection, the 2D generalization of [`List`]
//!   that reuses the [`Constraint`](rstui_core::Constraint) layout divider for
//!   column widths.
//! - [`checkbox`]: [`Checkbox`] — a single-line labelled boolean control, the
//!   first of the interactive form-control family and the first widget to
//!   model a focus visual; a pure projection of caller-owned `checked` and
//!   `focused` state (focus *routing* is deliberately deferred, not smuggled
//!   in).
//! - [`button`]: [`Button`] — a single-line centred focusable *action* label,
//!   the first form control with **no data** — a pure projection of only a
//!   caller-owned `focused` `bool`; the press action is the reducer's concern.
//! - [`radio`]: [`Radio`] — a single-line labelled *exclusive-choice* control,
//!   the exclusive-selection sibling of [`Checkbox`]; a pure projection of
//!   caller-owned `selected` (the data, the [`List`]-style selection concept)
//!   and `focused`. Exactly-one-per-group is the caller's invariant, not the
//!   widget's (a `RadioGroup` convenience is a deliberately deferred additive).
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
pub mod button;
pub mod checkbox;
pub mod gauge;
pub mod list;
pub mod paragraph;
pub mod radio;
pub mod scrollbar;
pub mod spinner;
pub mod table;
pub mod tabs;

pub use block::{Block, BorderSet, BorderType, Borders, Padding};
pub use button::Button;
pub use checkbox::Checkbox;
pub use gauge::Gauge;
pub use list::{List, ListItem};
pub use paragraph::{Paragraph, Wrap};
pub use radio::Radio;
pub use scrollbar::{Scrollbar, ScrollbarOrientation};
pub use spinner::Spinner;
pub use table::{Row, Table};
pub use tabs::Tabs;
