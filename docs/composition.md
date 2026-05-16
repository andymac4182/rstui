# Composing an rstui application

rstui is **immediate-mode and pure-projection**: there is no retained
widget tree. You compose an app by owning all state in one model,
splitting the screen with `Layout`, and rendering pure widgets into the
resulting rectangles. This is the documented, enforced model
([ADR 0012](adr/0012-widget-composition-and-layout-model.md)); the
executable proof is `cargo run -p rstui-widgets --example gallery` (a
dynamic full-screen app exercising every widget — also a headless
snapshot test).

## The one rule

> **The model owns state. `view` reads it and never mutates. `update`
> is the only place state changes.**

Every widget is a *projection* of caller-owned state. `List` does not
own its selection; you do. `Editor` does not own its text; you hold a
`rstui_core::TextArea` and edit it in `update`. A widget handed a
`Buffer` at render time physically cannot mutate your model — that is
the property the whole framework is built on.

## The four moves

### 1. Split the screen with `Layout`

`rstui_core::Layout` divides a `Rect` into contiguous regions by
`Constraint` (`Length`/`Min`/`Max`/`Percentage`/`Ratio`/`Fill`).
Compose splits recursively for any 2-D layout; reach for `Grid`
(row×column tiling), `SplitPane` (a divider + two panes), or `Align`
(centre/align a sized child) when they read better.

```rust
fn view(&self, frame: &mut Frame) {
    let area = frame.area();
    // App shell: body fills, status bar pinned to the bottom row.
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
    // Body: sidebar | content.
    let cols = Layout::horizontal([Constraint::Length(28), Constraint::Min(0)]).split(rows[0]);
    frame.render_widget(self.sidebar_widget(), cols[0]);
    frame.render_widget(self.content_widget(), cols[1]);
    frame.render_widget(self.status_bar_widget(), rows[1]);
}
```

### 2. Render pure widgets, fed your model

Build the widget from model state every frame. It is cheap; the
`Terminal` diffs frames so only changed cells are written.

```rust
List::new(&self.items)
    .selected(Some(self.selected))   // caller-owned
    .offset(self.scroll.offset())    // caller-owned
    .highlight_style(theme.selection)
```

### 3. Nest with `Rect` accessors

Container/overlay widgets expose **pure geometry accessors** that
return the rectangle their children occupy — you render those children
yourself. This is how composition nests without a tree:

| Accessor | Returns |
|---|---|
| `Block::inner(area)` | the framed content rect |
| `SplitPane::split(area)` | `(left, right)` pane rects |
| `Grid::cell(area, r, c)` | one cell rect |
| `Form::layout(area)` | per-field control rects |
| `Modal::area(area)` / `inner` | the centred dialog / its content |
| `Select::panel(area)` | the open dropdown rect |
| `Popover::placement(anchor, area)` | the flipped popover rect |
| `Accordion::layout(area)` | each open section's body rect |
| `ScrollView::viewport(area)` | the visible content slice |

```rust
let card = Card::new().title("Profile");
let body = card.inner(area);          // pure — no render yet
frame.render_widget(card, area);      // draw the frame
frame.render_widget(self.profile_form(), body); // render into it
```

### 4. Focus and overlays are model state

Hold a `rstui_core::FocusRing` in your model. `update` moves focus on
`Tab`/click; `view` reads `ring.is_focused(id)` and passes
`.focused(bool)` into each control. A modal/command-palette pushes a
**focus scope** (`ring.push_scope(..)`) on open and pops it on close —
focus is trapped and restored with no framework machinery. Overlays
(`Modal`, `Drawer`, `Toast`, `Popover`, `Menu`, `CommandPalette`) are
**opaque**: they `clear_region` their rect so background content cannot
bleed through. "Is a dialog open" is just a field in your model; render
it last, over the body.

```rust
// update():
Msg::OpenPalette => { self.palette_open = true;
                      self.focus.push_scope(self.palette_ids()); }
Msg::ClosePalette => { self.palette_open = false; self.focus.pop_scope(); }
// view(): draw body, then if self.palette_open render the palette over it.
```

## A dynamic, full-screen app

Drive the app with the runtime's public `run`/`App` loop (the same one
the headless `Harness` tests use, so the app is TTY-free testable):

```rust
struct Gallery { /* all state: selections, TextArea, FocusRing, tick, open flags … */ }
impl App for Gallery {
    type Message = Msg;
    fn on_event(&self, ev: &Event) -> Option<Msg> { /* pure: map input → intent */ }
    fn update(&mut self, msg: Msg) -> Cmd<Msg> { /* sole mutation; return a tick Cmd for animation */ }
    fn view(&self, frame: &mut Frame) { /* the four moves above */ }
}
```

"Dynamic" is just reducer-owned state changing over time: a `tick`
field advanced by a timer `Cmd` animates `Spinner`/`Skeleton`/
`Sparkline`; typed keys edit a `TextEdit`/`TextArea`; arrows move
`List`/`Tree`/`Table` selection and a `ScrollState`; a key toggles
`Switch`/opens a `Drawer`. The widget never animates itself — it
projects the tick.

## Scrolling, streaming, long content

Keep a caller-owned `rstui_core::scroll::ScrollState` per scrollable
region (see [ADR 0012 §P0](adr/0012-widget-composition-and-layout-model.md)).
In `update`, after content grows call `on_content_change(len, viewport)`
for sticky-bottom-while-streaming (a chat transcript), `show(child_y,
child_h, viewport, len)` to scroll an item into view, `scroll_by(..)`
for wheel/keys. `ScrollView` is a pure clip of a pre-built content
buffer at that offset plus scrollbars — it owns nothing. For very long
transcripts, build only the visible item range into the content buffer
(caller-side windowing) — the pure-projection answer to virtualization.

## Totality

Every widget clips or no-ops on a tiny, zero, or oversized area and on
out-of-range state — never panics. Your `view` therefore composes
safely at any terminal size; resize handling is automatic (the next
frame just re-splits the new `area`).

## See also

- [ADR 0012](adr/0012-widget-composition-and-layout-model.md) — the
  recorded composition decision and the residual-gap roadmap.
- [ADR 0002](adr/0002-widget-crate-boundary.md) /
  [ADR 0004](adr/0004-focus-routing-architecture.md) — the crate
  boundary and the focus model this builds on.
- `cargo run -p rstui-widgets --example gallery` — the executable,
  headless-tested flagship that demonstrates all of the above.
- Per-widget examples: `cargo run -p rstui-widgets --example <name>_demo`.
