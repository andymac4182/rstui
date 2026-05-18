# Component library

Every widget in `rstui-widgets`, grouped the way the crate groups them. Each
one is a **pure projection** of caller-owned state: it implements
`rstui_core::Widget`, stamps glyphs through `Buffer::set_cell`/`set_str`, and
is total (degenerate input clips or no-ops, never panics). The crate is the
worked reference for building your own widget crate
([ADR 0002](../adr/0002-widget-crate-boundary.md)).

- Every widget has a runnable demo: `cargo run -p rstui-widgets --example <name>`.
- Every widget has a recorded GIF under [`media/`](media/) (regenerate with
  `cargo xtask record`, see [Recording](../recording.md)).
- The flagship `gallery` composes **all** of them in one Elm-loop app:
  `cargo run -p rstui-widgets --example gallery`.

## Reading a widget entry

Each entry states: **purpose**, **companion types**, **state model** (what
caller-owned state it projects — this is the part that matters most), the
**key builder API**, and the **demo** command.

> **State model legend**
> - *pure projection* — reads caller-owned state passed in (`selected`,
>   `focused`, an offset, a `TextEdit`…); the reducer owns and mutates it.
> - *pure layout* — computes child `Rect`s from an area; renders no app data.
> - *owns nothing* — purely decorative; configured by the caller, no state.

## Families

| Family | Widgets | Reference |
|--------|---------|-----------|
| **Core set** | Block, Paragraph, List, Tabs, Gauge, Scrollbar, Spinner, FpsCounter, Table, Checkbox, Button, Radio, Input, Modal, StatusBar, Toast, Tree, Select, Editor | [core-set.md](core-set.md) |
| **Rich rendering** | Markdown, Link, Diff, Mermaid, Structurizr, JsonCanvas, Extmark, LineNumberGutter | [rich-rendering.md](rich-rendering.md) |
| **Forms & data** | Slider, Switch, Form, MaskedInput, Sparkline, BarChart, Calendar, DatePicker, DescriptionList, Badge, Alert, Divider, DataTable | [forms-and-data.md](forms-and-data.md) |
| **Navigation & layout** | Menu, CommandPalette, Tooltip, Breadcrumb, SplitPane, Accordion, Card, Sidebar, Stepper, Pagination | [navigation-and-layout.md](navigation-and-layout.md) |
| **Overlays & control** | ScrollView, Grid, Align, Popover, Drawer, Skeleton, Avatar, Kbd, HelpOverlay, KeymapView, WhichKey, Flow | [overlays-and-control.md](overlays-and-control.md) |
| **Observability** | LineChart, Heatmap, Histogram, StatPanel, FlameGraph, TraceWaterfall, LogStream | [observability.md](observability.md) |
| **Charts & visualization** | Canvas, ScatterPlot, PieChart, RadarChart, BoxPlot, ViolinChart, Candlestick, Waterfall, Funnel, BulletChart, Treemap, Sankey, Gantt, CalendarHeatmap, StackedBarChart | [charts.md](charts.md) |

## Alphabetical index

| Widget | Family | Demo example |
|--------|--------|--------------|
| [Accordion](navigation-and-layout.md#accordion) | nav/layout | `accordion_demo` |
| [Alert](forms-and-data.md#alert) | forms/data | `alert_demo` |
| [Align](overlays-and-control.md#align) | overlays | `align_demo` |
| [Avatar](overlays-and-control.md#avatar) | overlays | `avatar_demo` |
| [Badge](forms-and-data.md#badge) | forms/data | `badge_demo` |
| [BarChart](forms-and-data.md#barchart) | forms/data | `bar_chart_demo` |
| [Block](core-set.md#block) | core | `block_demo` |
| [BoxPlot](charts.md#boxplot) | charts | `box_plot_demo` |
| [Breadcrumb](navigation-and-layout.md#breadcrumb) | nav/layout | `breadcrumb_demo` |
| [BulletChart](charts.md#bulletchart) | charts | `bullet_chart_demo` |
| [Button](core-set.md#button) | core | `button_demo` |
| [Calendar](forms-and-data.md#calendar) | forms/data | `calendar_demo` |
| [CalendarHeatmap](charts.md#calendarheatmap) | charts | `calendar_heatmap_demo` |
| [Candlestick](charts.md#candlestick) | charts | `candlestick_demo` |
| [Canvas](charts.md#canvas) | charts | `canvas_demo` |
| [Card](navigation-and-layout.md#card) | nav/layout | `card_demo` |
| [Checkbox](core-set.md#checkbox) | core | `checkbox_demo` |
| [CommandPalette](navigation-and-layout.md#commandpalette) | nav/layout | `command_palette_demo` |
| [DataTable](forms-and-data.md#datatable) | forms/data | `data_table_demo` |
| [DatePicker](forms-and-data.md#datepicker) | forms/data | `date_picker_demo` |
| [DescriptionList](forms-and-data.md#descriptionlist) | forms/data | `description_list_demo` |
| [Diff](rich-rendering.md#diff) | rich | `diff_demo` |
| [Divider](forms-and-data.md#divider) | forms/data | `divider_demo` |
| [Drawer](overlays-and-control.md#drawer) | overlays | `drawer_demo` |
| [Editor](core-set.md#editor) | core | `editor_demo` |
| [Extmark](rich-rendering.md#extmark) | rich | `extmark_demo` |
| [FlameGraph](observability.md#flamegraph) | observability | `flame_graph_demo` |
| [Flow](overlays-and-control.md#flow) | overlays | `flow_demo` |
| [Form](forms-and-data.md#form) | forms/data | `form_demo` |
| [Funnel](charts.md#funnel) | charts | `funnel_demo` |
| [Gantt](charts.md#gantt) | charts | `gantt_demo` |
| [Gauge](core-set.md#gauge) | core | `gauge_demo` |
| [Grid](overlays-and-control.md#grid) | overlays | `grid_demo` |
| [Heatmap](observability.md#heatmap) | observability | `heatmap_demo` |
| [HelpOverlay](overlays-and-control.md#helpoverlay) | overlays | `help_overlay_demo` |
| [Histogram](observability.md#histogram) | observability | `histogram_demo` |
| [Input](core-set.md#input) | core | `input_demo` |
| [JsonCanvas](rich-rendering.md#jsoncanvas) | rich | `json_canvas_demo` |
| [Kbd](overlays-and-control.md#kbd) | overlays | `kbd_demo` |
| [KeymapView](overlays-and-control.md#keymapview) | overlays | `keymap_view_demo` |
| [LineChart](observability.md#linechart) | observability | `line_chart_demo` |
| [LineNumberGutter](rich-rendering.md#linenumbergutter) | rich | `line_number_gutter_demo` |
| [Link](rich-rendering.md#link) | rich | `markdown_links_demo` |
| [List](core-set.md#list) | core | `list_demo` |
| [LogStream](observability.md#logstream) | observability | `log_stream_demo` |
| [Markdown](rich-rendering.md#markdown) | rich | `markdown_demo` |
| [MaskedInput](forms-and-data.md#maskedinput) | forms/data | `masked_input_demo` |
| [Menu](navigation-and-layout.md#menu) | nav/layout | `menu_demo` |
| [Mermaid](rich-rendering.md#mermaid) | rich | `mermaid_demo` |
| [Modal](core-set.md#modal) | core | `modal_demo` |
| [Pagination](navigation-and-layout.md#pagination) | nav/layout | `pagination_demo` |
| [Paragraph](core-set.md#paragraph) | core | `paragraph_demo` |
| [PieChart](charts.md#piechart) | charts | `pie_chart_demo` |
| [Popover](overlays-and-control.md#popover) | overlays | `popover_demo` |
| [RadarChart](charts.md#radarchart) | charts | `radar_chart_demo` |
| [Radio](core-set.md#radio) | core | `radio_demo` |
| [Sankey](charts.md#sankey) | charts | `sankey_demo` |
| [ScatterPlot](charts.md#scatterplot) | charts | `scatter_plot_demo` |
| [Scrollbar](core-set.md#scrollbar) | core | `scrollbar_demo` |
| [ScrollView](overlays-and-control.md#scrollview) | overlays | `scroll_view_demo` |
| [Select](core-set.md#select) | core | `select_demo` |
| [Sidebar](navigation-and-layout.md#sidebar) | nav/layout | `sidebar_demo` |
| [Skeleton](overlays-and-control.md#skeleton) | overlays | `skeleton_demo` |
| [Slider](forms-and-data.md#slider) | forms/data | `slider_demo` |
| [Sparkline](forms-and-data.md#sparkline) | forms/data | `sparkline_demo` |
| [Spinner](core-set.md#spinner) | core | `spinner_demo` |
| [FpsCounter](core-set.md#fpscounter) | core | `fps_counter_demo` |
| [SplitPane](navigation-and-layout.md#splitpane) | nav/layout | `split_pane_demo` |
| [StackedBarChart](charts.md#stackedbarchart) | charts | `stacked_bar_chart_demo` |
| [StatPanel](observability.md#statpanel) | observability | `stat_panel_demo` |
| [StatusBar](core-set.md#statusbar) | core | `status_bar_demo` |
| [Stepper](navigation-and-layout.md#stepper) | nav/layout | `stepper_demo` |
| [Structurizr](rich-rendering.md#structurizr) | rich | `structurizr_demo` |
| [Switch](forms-and-data.md#switch) | forms/data | `switch_demo` |
| [Table](core-set.md#table) | core | `table_demo` |
| [Tabs](core-set.md#tabs) | core | `tabs_demo` |
| [Toast](core-set.md#toast) | core | `toast_demo` |
| [Tooltip](navigation-and-layout.md#tooltip) | nav/layout | `tooltip_demo` |
| [TraceWaterfall](observability.md#tracewaterfall) | observability | `trace_waterfall_demo` |
| [Tree](core-set.md#tree) | core | `tree_demo` |
| [Treemap](charts.md#treemap) | charts | `treemap_demo` |
| [ViolinChart](charts.md#violinchart) | charts | `violin_chart_demo` |
| [Waterfall](charts.md#waterfall) | charts | `waterfall_demo` |
| [WhichKey](overlays-and-control.md#whichkey) | overlays | `which_key_demo` |

## Shared patterns

Every widget in this crate follows the same handful of rules — learn them once:

- **Builder API.** Construct with `new(...)`, configure with chained
  `#[must_use]` methods returning `Self`. Defaults are sensible.
- **Caller-owned state.** A widget never holds app state. `selected`,
  `offset`, `focused`, `open`, a `TextEdit`, a `ScrollState` — all passed in
  by the reducer, which is the only thing that mutates them.
- **`Rect` accessors for nesting.** Containers expose `.inner(area)` /
  `.layout(area)` / `.split(area)` so you render children into the returned
  `Rect`s. There is no child list on the widget.
- **Overlays are opaque via `clear_region`.** `Modal`, `Drawer`, `Popover`,
  `Select`, `CommandPalette`, `Toast`, `HelpOverlay`, `Tooltip` blank their
  rect first, then draw — so they sit cleanly over content. The *focus* half
  of a modal is your `FocusRing` scope stack (see [Core reference](../core-reference.md#focus)).
- **Symbols.** Decorative single-`char` scalars (borders, eighth-blocks,
  spinner frames) map 1:1 to a cell; semantic affordances (checkbox marks)
  take `Cow<str>` so they can be multi-cell and overridable.
- **Total.** Zero-area, oversized, out-of-range — every widget clips or
  no-ops and never panics.

Start with the [core set](core-set.md).
