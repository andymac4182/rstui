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

### Caller-owned caches: the answer to a per-frame *parse*

Windowing handles "too many rows". The dual problem is a widget whose
input is a *language* — `Markdown`, the `Mermaid`/`Structurizr`/JSON-Canvas
family — where deriving the output means **running a parser + a layout
engine over the whole source every frame** (`widget/markdown/render` ≈
1.5 ms, ~18 % of a 120 fps frame; see
[`docs/perf-review-3.md`](perf-review-3.md)). Immediate mode forbids a
retained tree, but the parse is an immutable, deterministic function of its
inputs, so the answer is the same `ScrollState` seam: a **caller-owned
cache** in your model that the widget *reads through*.

Hold a `rstui_widgets::MarkdownCache` (or `DiagramCache`) in your model
beside the `ScrollState`; attach it with `Markdown::cache(&self.md)` /
`.diagram_cache(&self.diagrams)`. The first frame at a given
`(source, width, …)` computes on the unchanged code path; every later
frame — including the second `lines()` a scroll-clamp screen does — is an
`O(1)` lookup. A hit and a miss are **byte-identical** (gate-enforced), and
with no cache attached the widget is exactly as before — purely additive
([ADR 0025](adr/0025-caller-owned-line-cache.md); both wrap the one
internal `LineCache`). This is the perf complement to windowing: window so
you don't *build* off-screen rows; cache so you don't *re-derive* the ones
you do.

## Mouse: clicks, drags, and reusable pointer gestures

The framework delivers `Event::Mouse(MouseEvent { kind, position, .. })`;
`kind` is `Down/Up/Drag(button)`, `Moved`, or `Scroll*`. Hit-testing is the
reducer's job — and it must test **what was actually rendered**, because a
real terminal does not always send an initial `Resize`, so a layout guessed
from a seed size mis-places every click.

**The geometry seam (always do this).** `view` already computes the `Rect`s
of every region. Record them, in `view`, into an interior-mutable
`Cell<Geom>` (a small `Copy` struct of the rects this frame laid out); the
reducer reads `self.geom.get()` and hit-tests against it. `view` stays a pure
projection (it only *writes its own* geometry cache); the reducer never
guesses a size. This is the one rstui mouse rule.

**Click vs. drag.** Defer the click to `Up`: on `Down` record the press
cell, on `Drag` set a `moved` flag, on `Up` it is a *click* if `!moved`
else a *drag* finished. (Drag-select → copy is the worked case;
[ADR 0012 §P1](adr/0012-widget-composition-and-layout-model.md).)

**The reusable drag-and-drop recipe.** A whole press→drag→release gesture
(reorder a list, drag a card between columns, drag a pane divider) is plain
caller-owned state and a three-call seam:

1. Keep the in-flight gesture as `Option<Drag>` model state — `{ what was
   grabbed, where the pointer is }` — mutated only in `update`.
2. `on_press(pos)`: hit-test; if it grabs something, store the `Drag` and
   **return "handled"** to *claim the gesture*; else return "ignored" so the
   default behaviour (a click, or a text selection) still runs. Returning
   ignored by default means a component opts in **purely additively** —
   nothing else changes.
3. While claimed, the host routes `on_pointer_drag(pos)` (update
   `drag.at`) and `on_release(pos)` (commit the move, or cancel if it did
   not cross a valid target) to that component instead of the
   click/selection path.
4. `view` *projects* the gesture: dim the lifted item in place, draw a
   ghost following `drag.at`, highlight the drop target. The widget owns
   nothing; clamp the ghost to the area so it stays total.

Two worked, headless-tested references implement exactly this:

- the kitchen-sink **Kanban board** — drag a card across columns
  (`crates/rstui-kitchen-sink/src/screens/board.rs`, with the host seam in
  `screens/mod.rs` + `lib.rs`: `on_press` returning *handled* diverts
  `MouseDrag`/`MouseUp` from the selection machinery to the screen);
- **`rstui-git-review`** — drag the history/diff divider to resize and
  click a commit ([docs/git-review.md](git-review.md)), the same
  `Cell<Geom>` discipline in a standalone `App`.

Copy either: the seam is the same whether the host is the kitchen-sink
shell routing to screens or an `App` handling `Event::Mouse` directly.

### Mouse-resizable layout: the ready-made `SplitPane` seam

You do **not** hand-roll the pointer→size math for a draggable split.
[`SplitPane`](widgets/navigation-and-layout.md#splitpane) ships the seam —
the divider geometry *and* the conversion — as pure accessors; the split
position stays caller-owned model state (a `Constraint`):

```rust
// model: split: Constraint   (e.g. Constraint::Length(40))
let sp = SplitPane::new(self.split).block(/* … */);
let (left, right) = sp.split(area);            // render children into these
// in update(), on Event::Mouse against the area `view` recorded:
Down(p) if sp.contains_divider(area, p) => self.resizing = true,   // 1-cell grab tolerance
Drag(p) if self.resizing => self.split = sp.resize_to(area, p),    // pure, clamped, total
Up(_)   => self.resizing = false,
```

`resize_to` returns a clamped `Constraint::Length` (both panes stay ≥1
cell; a too-small area is a no-op), so it is stable under repeated drags
and total at any size. This is the generalisation of the bespoke divider
math `rstui-git-review` used — new code should use this seam, not re-derive
it.

### Widget review: what is mouse-friendly, and how

Every widget audited for pointer use. Widgets are pure projections, so
"mouse-friendly" means **the widget exposes the pure geometry seam** an
app's reducer needs (`Rect` accessors + a pointer→state converter); the
reducer owns the drag/selection.

| Class | Widgets | Seam |
|---|---|---|
| **Resizable / drag-to-set (drag seam)** | `SplitPane` | `contains_divider` + `resize_to` (above) |
| | `Scrollbar` / `ScrollView` | draggable thumb: `thumb_rect` + `position_at` (`*_scrollbar_rect` on `ScrollView`) |
| | `Slider` | drag-to-set: `track_rect` + `value_at` (pure, clamped to `min..=max`, both axes) |
| **Pure layout** (resize = change its input `Constraint`s, plain model state) | `Layout` (core), `Grid`, `Align`, `Flow`, `Card`, `Block` | `split` / `cell` / `inner` `Rect` accessors |
| **Click/drag hit seam** | `List` / `Tree` | `row_at(area,pos) -> Option<usize>` (border + scroll-offset aware) — click to select, press+drag to reorder |
| | `Tabs` | `tab_at(area,pos) -> Option<usize>` (variable-width, divider-aware — an even split mis-hits) |
| | `Table` | `column_rects(area)` + `header_cell_at(area,pos) -> Option<usize>` — press a header, release over another ⇒ the caller moves the column in its own `widths`+cells (pure projection: the order is yours) |
| | `DataTable` | `hit(area,pos) -> Header(col)` + `cell_rect`. Cells are **positional**, so a reorder must move the column slice **and** every row via `DataRow::move_cell(from,to)` **and** `DataTableState::reorder_column(from,to)` (it remaps the sort/group keys *and* an in-flight edit) — all the same `remove`+`insert`; cancel any open edit first. (Moving only the columns showed data under the wrong header — fixed.) |
| **Pointer-navigable** (app hit-tests a `Rect` accessor; click/drag, not resize) | `Accordion`, `Sidebar`, `Menu`, `Select`, `Pagination`, `Stepper`, `Calendar`/`DatePicker`, `Link`/`Markdown` (link regions), `Modal`/`Popover`/`Drawer` (`area`/`panel` focus accessors) | the screen maps a click to an index/region against the widget's geometry; full drag-and-drop uses the pointer-gesture recipe above |
| **Decorative** (no pointer surface by design) | `Paragraph`, `Gauge`, `Badge`, `Spinner`, `Skeleton`, `Divider`, charts, `StatusBar`, `Toast`, `Kbd`, `Avatar` | — |
| **Remaining backlog** (audited; no seam yet — each would add a `Rect` accessor + pointer→state converter, mirroring `SplitPane`/`Slider`) | **P1** `Table` column-*resize* drag — now small: `column_rects` already gives the boundaries, so it just needs a boundary-drag → per-column-width converter (column *reorder* shipped above); **P2** `Drawer` drag-the-edge resize; `Calendar`/`DatePicker` `day_at` (click a day / drag a date range); **P3** `Grid` resizable rows/columns (an N-way `SplitPane`); `Editor`/`TextArea` logical `DocSelection` drag-select (roadmap E1e — a *content* selection over the document, distinct from the screen-cell `Selection`) | tracked here so "what mouse work is left" is one answer, prioritised |

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
