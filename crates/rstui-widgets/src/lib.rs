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
//! - [`fps`]: [`FpsCounter`] — a live render-rate readout, a pure projection
//!   of a caller-owned [`FpsMeter`]; one line to make any app's frame
//!   performance visible (deterministic under the test harness).
//! - [`table`]: [`Table`] — a column-aligned grid of [`Row`]s with an optional
//!   fixed header and single-row selection, the 2D generalization of [`List`]
//!   that reuses the [`Constraint`](rstui_core::Constraint) layout divider for
//!   column widths.
//! - [`data_table`]: [`DataTable`] — the comprehensive interactive grid:
//!   sortable/filterable/groupable, mouse-hit-testable, virtualized for fast
//!   scroll, with any per-column cell control ([`CellField`]: text, checkbox,
//!   switch, dropdown — or any widget via [`cell_rect`](DataTable::cell_rect)).
//!   A pure projection of a
//!   caller-owned [`DataTableState`] (composing
//!   [`ScrollState`](rstui_core::ScrollState)) and a reducer-run
//!   [`project`](data_table::project) pipeline — the spreadsheet pane to
//!   [`Table`]'s aligned rows; change events surface as pure
//!   [`hit`](DataTable::hit) accessors, never callbacks (ADR 0012).
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
//! - [`mermaid`]: [`Mermaid`] — a narrow Mermaid flowchart subset parsed to a
//!   public AST ([`mermaid::MermaidGraph`]) and laid out as a deterministic
//!   Unicode box-and-arrow diagram.
//! - [`structurizr`]: [`Structurizr`] — a [Structurizr
//!   DSL](https://docs.structurizr.com/dsl) workspace parsed to a public C4
//!   model AST ([`structurizr::Workspace`]) and laid out as a deterministic
//!   C4 view (System Landscape / Context / Container / Component /
//!   Deployment) with stereotyped element cards, boundary boxes, and labelled
//!   relationship arrows. A separate diagramming language from Mermaid, so a
//!   separate widget.
//! - [`json_canvas`]: [`JsonCanvas`] — a [JSON
//!   Canvas 1.0](https://jsoncanvas.org/) document (the Obsidian
//!   infinite-canvas format) parsed by a hand-written zero-dep total JSON
//!   scanner to a public [`Canvas`](json_canvas::Canvas) AST and rendered with each node at its
//!   **explicit author-chosen `(x, y, width, height)`** scaled to fit — the
//!   *placement* complement to auto-layout Mermaid/Structurizr, the format
//!   an AI tool emits when it wants to control the layout.
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
//! - [`canvas`]: [`Canvas`]/[`Marker`]/[`Points`]/[`CanvasLine`]/[`Rectangle`] —
//!   the keystone free-form plotting surface: a [`paint`](canvas::Canvas::paint)
//!   closure draws caller-owned data in Cartesian space at sub-cell
//!   [`Marker`] resolution (Braille `2×4`, half-block, dot, block); the
//!   foundation the line/scatter charts plot on, immediate-mode (no retained
//!   scene), a pure projection with an optional [`Block`].
//! - [`scatter_plot`]: [`ScatterPlot`] (+ `scatter_plot::Series`) — an X/Y
//!   point cloud inside auto-fitting framed axes; composes [`Canvas`] for the
//!   cloud, a pure projection of caller-owned `&[(f64, f64)]` series.
//! - [`pie_chart`]: [`PieChart`]/[`Slice`] — a proportional disc or donut of
//!   coloured wedges with an optional legend; a pure projection, computes the
//!   proportions from caller-owned slice values.
//! - [`radar_chart`]: [`RadarChart`]/[`RadarAxis`]/[`RadarSeries`] — a
//!   spider plot of N axes with ring gridlines and series polygons (composes
//!   [`Canvas`]); a pure projection of caller-owned per-axis series.
//! - [`box_plot`]: [`BoxPlot`]/[`BoxStats`]/[`BoxPlotOrientation`] — a
//!   box-and-whisker over a shared scale; a pure projection of caller-owned
//!   five-number summaries (no statistics computed).
//! - [`candlestick`]: [`Candlestick`]/[`Candle`] — an OHLC financial chart
//!   with eighth-block sub-cell bodies and a price axis; a pure projection of
//!   a caller-owned `&[Candle]`.
//! - [`waterfall`]: [`Waterfall`]/[`WaterfallStep`]/[`WaterfallKind`]/[`WaterfallDirection`] —
//!   a financial bridge: signed steps float from the running cumulative with
//!   absolute totals and connectors; a pure projection of signed steps.
//! - [`funnel`]: [`Funnel`]/[`FunnelStage`] — a conversion funnel of centred
//!   bands sized by stage value with derived percentages; a pure projection.
//! - [`bullet_chart`]: [`BulletChart`]/[`Bullet`]/[`BulletChartDirection`] —
//!   Stephen Few's bullet graph (measure bar over qualitative bands + target
//!   tick), the compact KPI strip; a pure projection at sub-cell precision.
//! - [`treemap`]: [`Treemap`]/[`TreemapTile`] — area-proportional squarified
//!   tiling; a pure projection of caller-owned weighted tiles.
//! - [`sankey`]: [`Sankey`]/[`SankeyNode`]/[`SankeyLink`] — a left→right flow
//!   diagram (throughput-sized node bars, proportional link bands, composes
//!   [`Canvas`]); a pure projection of caller-owned nodes + links.
//! - [`gantt`]: [`Gantt`]/[`GanttTask`] — a project timeline (one bar per
//!   task on a shared axis, progress fill, today marker); a pure projection,
//!   no date math (the [`Calendar`] discipline).
//! - [`calendar_heatmap`]: [`CalendarHeatmap`] — a GitHub-style contribution
//!   calendar (weeks × weekdays, intensity-ramped); a pure projection of a
//!   caller-owned `&[u64]` day series, no date math.
//! - [`stacked_bar_chart`]: [`StackedBarChart`]/[`StackedBar`]/[`StackMode`] —
//!   multi-series **stacked**/**grouped** bars, the [`BarChart`] composition
//!   additive; a pure projection at eighth-block precision.
//! - [`violin_chart`]: [`ViolinChart`]/[`Violin`]/[`ViolinOrientation`] — a
//!   density (violin) plot, the distribution-*shape* sibling of [`BoxPlot`];
//!   a pure projection of a caller-computed density profile at eighth-block
//!   sub-cell thickness.
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
//! - [`scroll_view`]: [`ScrollView`] — the keystone scroll primitive: clips a
//!   borrowed pre-rendered content [`Buffer`](rstui_core::Buffer) to a window
//!   from a caller-owned 2D offset and draws a [`Scrollbar`] per overflowing
//!   axis; a pure projection, immediate-mode-correct (no negative translate).
//! - [`grid`]: [`Grid`] — a 2-D layout primitive: reuses core
//!   [`Layout`](rstui_core::Layout) per axis to tile an area into cells;
//!   `split`/`cell` accessors, owns no state (the [`SplitPane`] discipline).
//! - [`align`]: [`Align`]/[`VerticalAlignment`] — centres/aligns a
//!   [`Constraint`](rstui_core::Constraint)-sized child rect on both axes
//!   (the [`Modal`] centring math generalized); a pure accessor, not a modal.
//! - [`popover`]: [`Popover`]/[`PopoverSide`] — the generic anchored opaque
//!   floating panel [`Tooltip`]/[`Menu`]/[`Select`] specialize; side flip to
//!   stay on-buffer, a pure `placement` accessor.
//! - [`drawer`]: [`Drawer`]/[`DrawerSide`] — an edge-anchored opaque side
//!   sheet with optional backdrop; caller-owned `open` + size
//!   [`Constraint`](rstui_core::Constraint).
//! - [`sidebar`]: [`Sidebar`]/[`SidebarItem`] — an app navigation rail
//!   (collapsible groups, a narrow/expanded mode) reusing [`List`]; a pure
//!   projection of caller-owned `selected`/`collapsed`.
//! - [`skeleton`]: [`Skeleton`]/[`SkeletonShape`] — a loading placeholder
//!   whose shimmer is a pure projection of a caller-owned tick (the
//!   [`Spinner`] precedent, no wall clock).
//! - [`avatar`]: [`Avatar`] — a small initials swatch on an accent fill, a
//!   leaf pure projection.
//! - [`kbd`]: [`Kbd`] — an inline keycap glyph cluster (e.g. `⌃⇧P`); a leaf
//!   pure projection.
//! - [`help_overlay`]: [`HelpOverlay`]/[`HelpEntry`] — a centred opaque
//!   keybinding cheat-sheet reusing [`Kbd`] + clear-region (the [`Modal`]
//!   precedent, its own type).
//! - [`pagination`]: [`Pagination`] — a windowed pager (`‹ 1 … 4 [5] 6 … ›`),
//!   a pure projection of caller-owned `page`/`page_count`; a leaf.
//! - [`stepper`]: [`Stepper`]/[`Step`]/[`StepperOrientation`] — a
//!   horizontal/vertical wizard progress, a pure projection of caller-owned
//!   `current`.
//! - [`masked_input`]: [`MaskedInput`] — the [`Input`] projection with a mask
//!   glyph + unmask toggle (password fields); borrows a caller-owned
//!   [`TextEdit`](rstui_core::TextEdit), [`Input`] itself untouched.
//! - [`date_picker`]: [`DatePicker`] — a closed field that drops an opaque
//!   anchored [`Calendar`] panel (the [`Select`] anchored-panel idiom,
//!   self-contained); caller-owned open/selected day numbers, no date math.
//! - [`extmark`]: [`Extmark`] — a caller-owned `(range, Style, atomic)`
//!   overlay [`Input`] (and `rstui-code`'s `Editor`) project as styled,
//!   optionally cursor-atomic "pills" (@-mention/paste chips); the reducer
//!   owns and re-derives the ranges, the widget only reads (ADR 0012 §P1).
//! - [`flow`]: [`Flow`] — a wrapped horizontal run of [`Line`](rstui_core::Line)
//!   items packed across rows within the area with a configurable gap (the
//!   `flex-wrap` pill-row); a pure layout projection with a `layout` accessor.
//!
//! The **observability** family — the metrics / traces / logs primitives an
//! OpenTelemetry-style dashboard is built from, every one the same pure
//! projection of caller-owned series the rest of the catalog is:
//!
//! - [`line_chart`]: [`LineChart`]/[`Series`]/[`AxisBounds`] — a multi-series
//!   time-series XY plot with framed axes and a legend; the "metric over time"
//!   panel [`Sparkline`] is the one-row glance of.
//! - [`heatmap`]: [`Heatmap`] — a 2-D value grid mapped to a shade or colour
//!   ramp (latency-over-time, per-service error density); a flat row-major
//!   `&[f64]` + a column count, total on a short final row.
//! - [`histogram`]: [`Histogram`]/[`HistogramBucket`]/[`Percentile`] — a
//!   bucketed distribution with `p50`/`p95`/`p99` marker overlays, the
//!   distribution sibling of categorical [`BarChart`] (the shared eighth ramp).
//! - [`stat_panel`]: [`StatPanel`]/[`Trend`] — the single big KPI with a
//!   caption, a trend delta, and an inline sparkline backdrop; the
//!   observability tile [`Card`] generalizes to.
//! - [`flame_graph`]: [`FlameGraph`]/[`FlameFrame`] — a flame/icicle graph of
//!   a caller-owned **flattened** frame list (the [`Tree`] discipline) for
//!   CPU / trace profiles.
//! - [`trace_waterfall`]: [`TraceWaterfall`]/[`TraceSpan`] — a distributed
//!   trace span waterfall on a shared time axis (the [`BarChart`] sub-cell
//!   ramp), spans flattened in display order like [`Tree`].
//! - [`log_stream`]: [`LogStream`]/[`LogRecord`]/[`LogLevel`]/[`LogPalette`] —
//!   a structured, severity-coloured log viewer projecting a caller-owned
//!   scroll `offset` exactly like [`List`].
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
pub mod agenda_view;
pub mod alert;
pub mod align;
pub mod avatar;
pub mod badge;
pub mod bar_chart;
pub mod block;
pub mod box_plot;
pub mod breadcrumb;
pub mod bullet_chart;
pub mod button;
pub mod calendar;
pub mod calendar_heatmap;
pub mod candlestick;
pub mod canvas;
pub mod card;
pub mod checkbox;
pub mod command_palette;
pub mod data_table;
pub mod date_navigator;
pub mod date_picker;
pub mod day_view;
pub mod description_list;
/// The shared diagram drawing surface reused by the Mermaid and Structurizr
/// renderers. Crate-internal, not part of the public API.
mod diagram;
pub mod diagram_cache;
pub mod divider;
pub mod drawer;
pub mod event;
pub mod event_card;
pub mod event_editor;
pub mod extmark;
pub mod flame_graph;
pub mod flow;
pub mod form;
pub mod fps;
pub mod funnel;
pub mod gantt;
pub mod gauge;
pub mod grid;
pub mod heatmap;
pub mod help_overlay;
pub mod histogram;
pub mod input;
pub mod json_canvas;
pub mod kbd;
pub mod keymap_view;
mod line_cache;
pub mod line_chart;
pub mod link;
pub mod list;
pub mod log_stream;
pub mod markdown;
pub mod markdown_cache;
pub mod masked_input;
pub mod menu;
pub mod mermaid;
pub mod modal;
pub mod month_view;
pub mod pagination;
pub mod paragraph;
pub mod pie_chart;
pub mod popover;
pub mod projection_cache;
pub mod radar_chart;
pub mod radio;
pub mod sankey;
pub mod scatter_plot;
pub mod scroll_view;
pub mod scrollbar;
pub mod select;
pub mod sidebar;
pub mod skeleton;
pub mod slider;
pub mod sparkline;
pub mod spinner;
pub mod split_pane;
pub mod stacked_bar_chart;
pub mod stat_panel;
pub mod status_bar;
pub mod stepper;
pub mod structurizr;
pub mod switch;
pub mod table;
pub mod tabs;
pub mod time_picker;
pub mod toast;
pub mod tooltip;
pub mod trace_waterfall;
pub mod tree;
pub mod treemap;
pub mod violin_chart;
pub mod waterfall;
pub mod week_view;
pub mod which_key;
pub mod year_view;

pub use accordion::{Accordion, AccordionSection};
pub use alert::{Alert, AlertLevel};
pub use badge::{Badge, BadgeLevel};
pub use bar_chart::{Bar, BarChart, BarChartDirection};
pub use block::{Block, BorderSet, BorderType, Borders, Padding};
pub use breadcrumb::Breadcrumb;
pub use button::Button;
pub use calendar::Calendar;
pub use canvas::{Canvas, CanvasLine, Context, Marker, Painter, Points, Rectangle, Shape};
pub use card::Card;
pub use checkbox::Checkbox;
pub use command_palette::CommandPalette;
pub use data_table::{
    CellField, CellSelectState, DataColumn, DataRow, DataTable, DataTableHit, DataTableState,
    RowSource, SortDirection, VisualRow, cell_truthy,
};
pub use description_list::{DescriptionList, DescriptionRow};
pub use diagram_cache::DiagramCache;
pub use divider::{Divider, DividerOrientation};
pub use extmark::Extmark;
pub use form::{Form, FormField};
pub use fps::{FpsCounter, FpsMeter};
pub use gauge::Gauge;
pub use input::Input;
pub use link::{Link, LinkActivation};
pub use list::{List, ListItem};
pub use markdown::{LinkRegion, Markdown, MarkdownTheme};
pub use markdown_cache::MarkdownCache;
pub use menu::{Menu, MenuItem};
pub use projection_cache::ProjectionCache;
// The Mermaid AST types (`Direction`, `Node`, `Edge`, `EdgeKind`, `Shape`,
// `MermaidGraph`) are intentionally reached via `mermaid::` rather than
// re-exported at the crate root: `Direction`/`Node`/`Edge` are generic enough
// to collide with `rstui_core` and future widgets, so only the widget and its
// configuration/error surface are promoted.
pub use align::{Align, VerticalAlignment};
pub use avatar::Avatar;
pub use date_picker::DatePicker;
pub use drawer::{Drawer, DrawerSide};
pub use flame_graph::{FlameFrame, FlameGraph};
pub use flow::Flow;
pub use grid::Grid;
pub use heatmap::Heatmap;
pub use help_overlay::{HelpEntry, HelpOverlay};
pub use histogram::{Histogram, HistogramBucket, Percentile};
pub use json_canvas::{JsonCanvas, JsonCanvasError, JsonCanvasTheme};
pub use kbd::Kbd;
pub use keymap_view::{KeymapRow, KeymapView, RowState};
pub use line_chart::{AxisBounds, LineChart, Series};
pub use log_stream::{LogLevel, LogPalette, LogRecord, LogStream};
pub use masked_input::MaskedInput;
pub use mermaid::{Mermaid, MermaidError, MermaidTheme};
pub use modal::Modal;
pub use pagination::Pagination;
pub use paragraph::{Paragraph, Wrap};
pub use popover::{Popover, PopoverSide};
pub use radio::Radio;
pub use scroll_view::ScrollView;
pub use scrollbar::{Scrollbar, ScrollbarOrientation};
pub use select::Select;
pub use sidebar::{Sidebar, SidebarItem};
pub use skeleton::{Skeleton, SkeletonShape};
pub use slider::{Slider, SliderOrientation};
pub use sparkline::Sparkline;
pub use spinner::Spinner;
pub use split_pane::SplitPane;
pub use stat_panel::{StatPanel, Trend};
pub use status_bar::StatusBar;
pub use stepper::{Step, Stepper, StepperOrientation};
pub use structurizr::{Structurizr, StructurizrError, StructurizrTheme};
pub use switch::Switch;
pub use table::{Row, Table, TableColumnFit};
pub use tabs::Tabs;
pub use toast::{Toast, ToastCorner, ToastLevel, ToastMessage};
pub use tooltip::Tooltip;
pub use trace_waterfall::{TraceSpan, TraceWaterfall};
pub use tree::{Tree, TreeGuides, TreeItem};
pub use which_key::WhichKey;
// The business-dashboard chart cluster — a later additive export wave (like
// the group above). `scatter_plot::Series` and the Mermaid AST stay
// module-qualified; `Series` is too generic to promote to the crate root.
pub use box_plot::{BoxPlot, BoxPlotOrientation, BoxStats};
pub use bullet_chart::{Bullet, BulletChart, BulletChartDirection};
pub use calendar_heatmap::CalendarHeatmap;
pub use candlestick::{Candle, Candlestick};
pub use funnel::{Funnel, FunnelStage};
pub use gantt::{Gantt, GanttTask};
pub use pie_chart::{PieChart, Slice};
pub use radar_chart::{RadarAxis, RadarChart, RadarSeries};
pub use sankey::{Sankey, SankeyLink, SankeyNode};
pub use scatter_plot::ScatterPlot;
pub use stacked_bar_chart::{StackMode, StackedBar, StackedBarChart};
pub use treemap::{Treemap, TreemapTile};
pub use violin_chart::{Violin, ViolinChart, ViolinOrientation};
pub use waterfall::{Waterfall, WaterfallDirection, WaterfallKind, WaterfallStep};
// The calendar-app widget family — a later additive export wave (the pattern
// the observability and business-dashboard clusters use). Every view is a
// pure projection of the shared caller-owned [`CalendarEvent`] model; the
// app owns moving/scheduling via the views' `*_at` hit accessors (ADR 0026).
pub use agenda_view::AgendaView;
pub use date_navigator::{DateNavigator, NavTarget};
pub use day_view::DayView;
pub use event::{CalendarEvent, EventLayout};
pub use event_card::EventCard;
pub use event_editor::{EventEditor, EventEditorField};
pub use month_view::MonthView;
pub use time_picker::TimePicker;
pub use week_view::WeekView;
pub use year_view::YearView;
