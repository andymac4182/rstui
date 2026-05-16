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
//! - [`input`]: [`Input`] — a single-line text-entry field, the first
//!   text-edit/cursor widget and the first [`focus`](rstui_core::focus)
//!   consumer; a pure projection of a borrowed caller-owned
//!   [`TextEdit`](rstui_core::TextEdit) plus `focused`, with a rendered (not
//!   terminal) caret and a stateless caret-following horizontal scroll.
//! - [`markdown`]: [`Markdown`] — a read-only document view that parses a
//!   CommonMark-ish subset (headings, emphasis, code, quotes, lists, tables,
//!   rules, links) with a hand-written zero-dependency parser and lays it out
//!   width-aware into the styled-text model (ADR 0002 §4: a grammar is not a
//!   "heavy, alien" dependency, so it is a plain module here, not a feature or
//!   crate). The rich-rendering family below shares that hand-written ethos.
//! - [`link`]: [`Link`] / [`LinkActivation`] — the link-span model and its
//!   activation event shape; documents expose links in reading order and the
//!   app owns the focused index, the same pure-projection discipline as
//!   [`List`] selection (activation is the reducer's concern, not smuggled in).
//! - [`diff`]: [`Diff`] — a unified-diff view (hunk headers, +/- gutters,
//!   line numbers, word-level intra-line highlight), the document analogue of
//!   `Paragraph` for code review panes.
//! - [`mermaid`]: [`Mermaid`] — a narrow Mermaid flowchart subset parsed to a
//!   public AST ([`mermaid::MermaidGraph`]) and laid out as a deterministic
//!   Unicode box-and-arrow diagram.
//! - [`modal`]: [`Modal`] — a centred, **opaque**, optionally-[`Block`]-framed
//!   dialog over an overlay area; the visual half of the
//!   [`FocusRing`](rstui_core::FocusRing) scope-stack modal model (ADR 0004
//!   §6). A pure projection — the app decides "is a modal open" in its model
//!   and `view` renders it; the widget never reads focus.
//! - [`status_bar`]: [`StatusBar`] — a one-row strip with independently
//!   left-/centre-/right-anchored [`Line`](rstui_core::Line) segments and a
//!   fixed, documented contention rule; the first multi-anchor layout widget,
//!   a pure projection of three caller-built segments (the editor/file-manager
//!   status strip).
//! - [`toast`]: [`Toast`] — a corner-anchored, **opaque** stack of transient
//!   [`ToastMessage`] notifications (per-[`ToastLevel`] accents, an optional
//!   framing [`Block`]) floated over an overlay; a pure projection of a
//!   caller-owned message list — expiry/dismissal is the reducer's, like
//!   [`Modal`]'s clear-region opacity and [`Paragraph`]'s reused soft wrap.
//! - [`tree`]: [`Tree`] — a single-select column of indented,
//!   expand/collapse rows ([`TreeItem`]/[`TreeGuides`]); the [`List`]
//!   projection generalized to a caller-owned **flattened** `Vec` of
//!   currently-visible rows (the reducer owns the tree, expansion, and
//!   `selected`/`offset`; the widget only reads them).
//! - [`select`]: [`Select`] — a single-line dropdown: a closed field that
//!   drops an **opaque**, field-anchored option panel when the caller-owned
//!   `open` flag is set; the first *floating* control. A pure projection of
//!   caller-owned `open`/`selected`/`highlight`/`offset` that **reuses**
//!   [`List`] for the panel and [`Modal`]'s clear-region opacity (but is
//!   deliberately not a [`Modal`] — anchored, not modal/centred).
//! - [`editor`]: [`Editor`] — a multi-line text-entry widget, the [`Input`]
//!   dual for documents; a pure projection of a borrowed caller-owned
//!   [`TextArea`](rstui_core::TextArea) model plus caller-owned 2D `scroll`
//!   and `focused`, with a rendered (not terminal) 2D caret. The reducer owns
//!   the edit and the scroll (ADR 0004 §1); the widget only reads.
//! - [`slider`]: [`Slider`] — a horizontal value selector, a pure projection
//!   of a caller-owned `value` in `min..=max` plus `focused`; sub-cell
//!   precision (the [`Gauge`] eighth-block ramp). A leaf control, no `Block`.
//! - [`switch`]: [`Switch`] — a two-state toggle with a sliding track, the
//!   [`Checkbox`] focus/cascade idiom; a pure projection of caller-owned
//!   `on`/`focused`. A leaf control.
//! - [`form`]: [`Form`]/[`FormField`] — a pure **layout** projection that owns
//!   no app state: it lays out label+control+help rows and exposes a
//!   per-field control `Rect` (the [`Modal::inner`](modal::Modal) pattern), so
//!   the caller renders its own controls.
//! - [`menu`]: [`Menu`]/[`MenuItem`] — an **opaque** action list (key hints,
//!   separators, disabled rows) reusing [`List`]; commits an action via the
//!   reducer (the [`Select`]-not-[`Modal`] precedent).
//! - [`command_palette`]: [`CommandPalette`] — the worked **composition**:
//!   [`Input`] + filtered [`List`] + [`Block`] + clear-region in a centred
//!   panel; a pure projection (the reducer filters, not the widget).
//! - [`tooltip`]: [`Tooltip`] — a small opaque popup anchored to a caller
//!   `Rect`, flipping side to stay on-buffer (the [`Select`] placement rule);
//!   a pure projection with a `placement` accessor.
//! - [`breadcrumb`]: [`Breadcrumb`] — a one-row path strip (` › ` joiner,
//!   last/selected emphasized, middle elided to `…` when narrow); a leaf, the
//!   [`StatusBar`] precedent.
//! - [`sparkline`]: [`Sparkline`] — a one-row trend of caller data via the
//!   eight vertical block glyphs; a pure projection, a leaf control.
//! - [`bar_chart`]: [`BarChart`]/[`Bar`]/[`BarChartDirection`] — labelled
//!   horizontal/vertical bars at sub-cell precision (the [`Gauge`] ramp); a
//!   pure projection with an optional [`Block`].
//! - [`calendar`]: [`Calendar`] — a month grid that does **no** date math
//!   (the caller supplies the day numbers — dependency-free, no `chrono`); a
//!   pure projection with optional [`Block`].
//! - [`description_list`]: [`DescriptionList`]/[`DescriptionRow`] — an aligned
//!   key→value pane; values wrap by reusing [`Paragraph`] (no second wrap).
//! - [`badge`]: [`Badge`]/[`BadgeLevel`] — a tiny inline status pill with
//!   per-level accents; a pure projection, a leaf control.
//! - [`alert`]: [`Alert`]/[`AlertLevel`] — a persistent (non-transient,
//!   unlike [`Toast`]) framed banner; body wrap reuses [`Paragraph`].
//! - [`divider`]: [`Divider`]/[`DividerOrientation`] — a horizontal/vertical
//!   rule with an optional label; a pure projection, a leaf control.
//! - [`split_pane`]: [`SplitPane`] — splits an area into two panes by a
//!   caller-owned [`Constraint`](rstui_core::Constraint) with a divider glyph;
//!   exposes `split`/`inner` accessors (pure layout, owns no state).
//! - [`accordion`]: [`Accordion`]/[`AccordionSection`] — a stack of titled
//!   collapsible sections; a pure layout projection of caller-owned
//!   `expanded` flags, exposing each open body `Rect`.
//! - [`card`]: [`Card`] — a titled container, a thin convenience composition
//!   over [`Block`] with header/footer lines and an `inner` body accessor.
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

pub mod accordion;
pub mod alert;
pub mod badge;
pub mod bar_chart;
pub mod block;
pub mod breadcrumb;
pub mod button;
pub mod calendar;
pub mod card;
pub mod checkbox;
pub mod command_palette;
pub mod description_list;
pub mod diff;
pub mod divider;
pub mod editor;
pub mod form;
pub mod gauge;
pub mod input;
pub mod link;
pub mod list;
pub mod markdown;
pub mod menu;
pub mod mermaid;
pub mod modal;
pub mod paragraph;
pub mod radio;
pub mod scrollbar;
pub mod select;
pub mod slider;
pub mod sparkline;
pub mod spinner;
pub mod split_pane;
pub mod status_bar;
pub mod switch;
pub mod table;
pub mod tabs;
pub mod toast;
pub mod tooltip;
pub mod tree;

pub use accordion::{Accordion, AccordionSection};
pub use alert::{Alert, AlertLevel};
pub use badge::{Badge, BadgeLevel};
pub use bar_chart::{Bar, BarChart, BarChartDirection};
pub use block::{Block, BorderSet, BorderType, Borders, Padding};
pub use breadcrumb::Breadcrumb;
pub use button::Button;
pub use calendar::Calendar;
pub use card::Card;
pub use checkbox::Checkbox;
pub use command_palette::CommandPalette;
pub use description_list::{DescriptionList, DescriptionRow};
pub use diff::{Diff, DiffLayout, DiffTheme};
pub use divider::{Divider, DividerOrientation};
pub use editor::Editor;
pub use form::{Form, FormField};
pub use gauge::Gauge;
pub use input::Input;
pub use link::{Link, LinkActivation};
pub use list::{List, ListItem};
pub use markdown::{LinkRegion, Markdown, MarkdownTheme};
pub use menu::{Menu, MenuItem};
// The Mermaid AST types (`Direction`, `Node`, `Edge`, `EdgeKind`, `Shape`,
// `MermaidGraph`) are intentionally reached via `mermaid::` rather than
// re-exported at the crate root: `Direction`/`Node`/`Edge` are generic enough
// to collide with `rstui_core` and future widgets, so only the widget and its
// configuration/error surface are promoted.
pub use mermaid::{Mermaid, MermaidError, MermaidTheme};
pub use modal::Modal;
pub use paragraph::{Paragraph, Wrap};
pub use radio::Radio;
pub use scrollbar::{Scrollbar, ScrollbarOrientation};
pub use select::Select;
pub use slider::Slider;
pub use sparkline::Sparkline;
pub use spinner::Spinner;
pub use split_pane::SplitPane;
pub use status_bar::StatusBar;
pub use switch::Switch;
pub use table::{Row, Table};
pub use tabs::Tabs;
pub use toast::{Toast, ToastCorner, ToastLevel, ToastMessage};
pub use tooltip::Tooltip;
pub use tree::{Tree, TreeGuides, TreeItem};
