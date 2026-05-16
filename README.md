# rstui

An idiomatic Rust TUI framework for building powerful terminal applications
quickly.

rstui learns from the architecture of OpenTUI, the real application needs of
OpenCode, the update/view/event-loop ergonomics of Bubble Tea, the terminal
rendering ecosystem of ratatui, and the breadth and polish of gpui-component —
while staying idiomatic to Rust.

> **Status:** early foundation. The rendering substrate, the terminal
> `Backend` boundary, the `EventSource` input boundary (with an in-memory
> `TestEventSource`), the double-buffered `Terminal` frame driver, the
> constraint-based `Layout` divider, the keyboard/mouse/focus/resize `Event`
> model, the `Widget` abstraction with the foundational `Block` container,
> the `Paragraph` text widget (word wrap, scroll, alignment), the
> scrollable single-select `List`, the horizontal `Tabs` strip, the
> sub-cell-precision `Gauge` progress bar, the `Scrollbar` scroll
> indicator, the animated `Spinner` busy indicator, the column-aligned
> `Table` grid (optional header, single-row selection), the labelled
> boolean `Checkbox` control, the centred focusable `Button` action
> label, the exclusive-choice `Radio` control and the single-line
> text-entry `Input` field (the form-control family, with a focus
> visual), the centred opaque `Modal` dialog (the visual half of the
> modal-focus model, over the new `Buffer::clear_region` overlay
> primitive), the optional caller-owned `focus` model (`FocusId` value
> tokens plus a pure, total, wrapping `FocusRing` with a model-owned
> modal **focus-scope stack** — `push_scope`/`pop_scope`, validated
> capture/restore, declarative reducer-gated trapping) those controls'
> `focused: bool` projects from, the optional caller-owned single-line
> text-editing model (`TextEdit`, a pure, total
> `String`+character-cursor value the `Input` widget projects with a
> rendered caret), the styled-text
> model (`Span`/`Line`/`Text`) with the `Stylize` fluent shorthand
> (`"x".green().bold()`, `.on_blue()`), the Elm-style
> `App`/`Cmd`/`Harness` runtime **plus the live `run` loop (the production
> twin of `Harness`, generic over any `Backend` + `EventSource`)**, and the
> crossterm terminal driver's input translation, `Backend` implementation,
> panic-safe RAII lifecycle guard, **and `CrosstermEventSource` input source**
> exist and are tested. **The framework now composes end to end — the same
> `run` the headless tests drive runs an unmodified app on a real terminal**
> (the `run_app` example). A broader component set and the plugin host are
> not built yet.

## Workspace

The project is a Cargo workspace (Rust 2024 edition). Crates are introduced
only when there is enough real API surface to justify the boundary.

| Crate                  | Responsibility                                                                 |
| ---------------------- | ------------------------------------------------------------------------------ |
| `crates/rstui-core`      | Dependency-free substrate: geometry, style, stylize, layout, buffer, backend, terminal, event, event_source, focus, the `Widget` trait, text, text_edit |
| `crates/rstui-widgets`   | The concrete widget set ([ADR 0002](docs/adr/0002-widget-crate-boundary.md)), one module per widget — `Block`, `Paragraph`, `List`, `Tabs`, `Gauge`, `Scrollbar`, `Spinner`, `Table`, `Checkbox`, `Button`, `Radio`, `Input`, `Modal`, and `StatusBar` today. Depends only on `rstui-core`; the worked reference for third-party widget crates |
| `crates/rstui-runtime`   | Elm-style `App`/`Cmd` contract, a deterministic terminal-free test harness, and the live `run` loop they share |
| `crates/rstui-crossterm` | The crossterm-backed terminal driver ([ADR 0001](docs/adr/0001-terminal-backend-strategy.md)); the workspace's only external dependency, isolated here. The crossterm → `rstui-core` event translation, the `Backend` impl over `io::Write`, the panic-safe RAII lifecycle guard, and the `CrosstermEventSource` input source |
| `crates/xtask`           | Workspace automation (the cargo-xtask convention; dependency-free). Hosts `lint-names`, the project-specific [vague-generic-naming gate](docs/conventions/naming.md) ([ADR 0003](docs/adr/0003-lint-and-code-quality-policy.md) §7) — the one defect class clippy/rustdoc cannot see |

The `rstui-crossterm` crate ([ADR 0001](docs/adr/0001-terminal-backend-strategy.md))
is now complete for the synchronous path: its crossterm → `rstui-core`
event-translation layer, its `Backend` implementation over `io::Write`, its
panic-safe terminal-lifecycle guard, and its `CrosstermEventSource`
(`rstui-core`'s `EventSource` over sync `poll`/`read`) have all landed. With
the live `run` loop in `rstui-runtime`, the whole framework composes end to
end: the `run_app` example runs an unmodified `App` on a real terminal via
the *same* `run` the headless harness tests drive. A feature-gated async
`EventStream` source is a future enhancement. Other planned
boundaries as the framework grows: a broader component set (the `Widget`
trait stays in core; concrete widgets live in the grouped `rstui-widgets`
crate per [ADR 0002](docs/adr/0002-widget-crate-boundary.md), now extracted
— `Block`, `Paragraph`, `List`, `Tabs`, `Gauge`, `Scrollbar`, `Spinner`,
`Table`, `Checkbox`, `Button`, `Radio`, `Input`, `Modal`, and `StatusBar`
ship there
today, with `Buffer::set_cell` the public cell-stamping contract third-party widgets
build on; `Alignment`
stays in `rstui-core::layout` as the placement primitive the text model
needs), more widgets and examples, and a permissioned plugin host built on
process isolation.

### `rstui-core`

`rstui-core` is pure and deterministic — no terminal, async runtime, or event
loop — so every layer above it can be unit tested without a TTY.

- `geometry` — `Position`, `Size`, `Rect`, `Margin`. `Rect::new` clamps so
  edge accessors can never overflow.
- `style` — `Color`, `Modifier`, and a composable `Style` patch model that
  themes, focus, and selection highlights build on.
- `stylize` — the `Stylize` extension trait: fluent `"x".green().bold()` /
  `.on_blue()` / `.not_bold()` shorthands over any `Styled` value (`&str`,
  `String`, `Span`, `Line`, `Text`, `Style`). One blanket impl over `Styled`,
  so a custom widget gets the whole vocabulary by implementing one trait.
- `layout` — `Layout`, `Direction`, and the `Constraint` vocabulary
  (`Length`/`Percentage`/`Ratio`/`Min`/`Max`/`Fill`) for dividing a `Rect`
  into contiguous regions. A deterministic, integer-only divider (no Cassowary
  solver, no floats) that always tiles the area exactly. Also `Alignment`
  (left/center/right) — the placement primitive the text model and widgets
  share, kept in core so the text model never reaches back into a widget crate
  (matching `ratatui_core::layout::Alignment`).
- `buffer` — `Cell` and the immediate-mode `Buffer` grid widgets draw into and
  renderers `diff` against; `Buffer::clear_region` is the opaque-overlay
  primitive (a true reset a style patch cannot express) floating widgets like
  `Modal` take exclusive ownership of their area through.
- `backend` — the `Backend` trait (the screen boundary that consumes a
  `Buffer` diff) and an in-memory `TestBackend` so UIs are testable without a
  TTY. Real terminal backends will live in their own crate.
- `terminal` — the `Terminal` frame driver: double buffers plus a
  `draw(|frame| …)` closure that diffs, flushes, places the cursor, and swaps
  so redraws are minimal and flicker-free. The seam a model/update/view
  runtime will sit on.
- `event` — the `Event` vocabulary (`KeyEvent`, `MouseEvent`, resize, focus,
  paste) the runtime, components, and focus routing share. Pure data shaped
  like the de-facto crossterm model so a real backend bridges 1:1, but using
  rstui's own `Position`/`Size`.
- `event_source` — the `EventSource` input boundary (the dual of `Backend`):
  one `poll_event(timeout)` call folding crossterm's `poll`+`read`, plus an
  in-memory `TestEventSource` that replays a scripted event stream so whole
  apps are drivable end-to-end with no TTY. The real terminal input source
  lives in the backend crate; the trait and the test source stay here.
- `focus` — the optional, caller-owned focus model
  ([ADR 0004](docs/adr/0004-focus-routing-architecture.md)): `FocusId`, an
  opaque `Copy` value-identity token the app mints, and `FocusRing`, a pure
  value type (explicit ordered ids + the focused one) that lives as a field in
  the app's model — `update` calls `focus`/`focus_next`/`focus_prev` (wrapping
  and **total**), `view` reads `is_focused`. `FocusRing` also carries a
  **modal scope stack** (ADR 0004 §6): `push_scope`/`pop_scope` trap focus to
  a modal's own ids (every traversal/lookup is scope-constrained while active),
  capture-and-validate-restore the prior focus, and expose
  `in_scope`/`scope_depth` so the reducer gates background input declaratively.
  An un-scoped ring is byte-for-byte the pre-scope behavior. Never runtime- or
  widget-owned, and distinct from terminal-window `FocusGained`/`FocusLost`.
  Not required — an app may use its own `enum`; this just removes the
  boilerplate.
- `widget` — the `Widget` trait (`render(self, area, buf)`) every component
  implements, plus blanket impls for `&str`/`String`/`Option<W>`. Only the
  trait lives here; the concrete widget set is the separate `rstui-widgets`
  crate so core stays primitives-only ([ADR 0002](docs/adr/0002-widget-crate-boundary.md)).
  `Frame::render_widget` is the entry point, and `Buffer::set_cell` is the
  public, bounds-safe cell-stamping contract a custom widget draws through.
- `text` — the styled-text model: `Span` (a styled run), `Line` (a row of
  spans with optional alignment), and `Text` (a block of lines). One
  committed, data-driven model with a predictable text→line→span `Style`
  cascade; `Cow<str>` content keeps literals allocation-free. Width is a
  `char` count, matching the single-`char` `Cell`; wrap/scroll live in the
  `Paragraph` widget, not these primitives.
- `text_edit` — the optional, caller-owned single-line editing model
  ([ADR 0004](docs/adr/0004-focus-routing-architecture.md) Follow-up §2):
  `TextEdit`, a pure value type (a `String` plus a **character-indexed**
  cursor) that lives as a field in the app's model — `update` calls
  `insert_char`/`insert_str`/`delete_backward`/`delete_forward`/`move_*`,
  the pure `view` reads `value`/`cursor` to project it through an `Input`
  widget. The editing-side dual of `FocusRing`: every method is **total**
  (a multi-byte paste or an out-of-range `set_cursor` never panics or
  strands the cursor mid-codepoint). Single-line by convention; not
  required — an app may keep its own `String`+`usize`.

### `rstui-widgets`

The concrete widget set, kept out of `rstui-core` so the universally
depended-on primitives crate stays small and slow-moving
([ADR 0002](docs/adr/0002-widget-crate-boundary.md)). One grouped crate,
**one module per widget** (not one crate per widget), depending only on
`rstui-core`. Because it uses nothing but `rstui-core`'s public API
(`impl rstui_core::Widget`, stamp through `Buffer::set_cell`, snapshot-test
against `TestBackend`), it is the worked reference a third-party widget
crate copies.

- `block` — `Block`: the foundational container — `Borders`, `BorderType`,
  `BorderSet`, `Padding`, a styled fill, and a clipped title that is a full
  `Line` (per-span styles and its own alignment, cascading over block-level
  `title_style`/`title_alignment`), with `Block::inner` handing the remaining
  area to the content drawn inside.
- `paragraph` — `Paragraph`: the multi-line text widget adding soft word
  `Wrap` (`trim` controls leading-whitespace handling; over-wide words hard
  split), a `Position` scroll offset, per-block alignment, and an optional
  framing `Block` — none of which leak into the core text primitives.
- `list` — `List`: a scrollable, single-select column of `ListItem` rows with
  a full-width highlight bar, a reserved highlight-symbol gutter, and an
  optional framing `Block`. A **pure projection** of caller-owned
  `selected`/`offset` state — never mutated at render time — so it composes
  with the Elm `view(&self)` model rather than needing ratatui's
  render-mutating `StatefulWidget`.
- `tabs` — `Tabs`: a one-row horizontal title strip with a configurable
  divider, an optional framing `Block`, and one `selected` title emphasised
  (the highlight covers the title glyphs only, not the padding/dividers). The
  same caller-owned **pure projection** as `List`, on the horizontal axis —
  concrete proof the projection model is axis-independent.
- `gauge` — `Gauge`: a horizontal progress bar with an optional framing
  `Block` and a centred label (the rounded percentage by default,
  colour-swapped for readability where it crosses the bar). The first
  **sub-cell-precision** widget: the fill boundary is drawn with the partial
  eighth-block glyph (`▏▎▍▌▋▊▉█`) nearest the true fraction, so the bar has
  `8·width` positions, not `width`. `ratio`/`percent` clamp instead of
  panicking — a gauge is a pure projection of a caller-owned number.
- `scrollbar` — `Scrollbar`: a track-and-thumb scroll indicator placed on any
  edge by `ScrollbarOrientation`. The visible companion to `List`/`Paragraph`
  scrolling — it reads the *same* caller-owned `position` they scroll by, the
  same caller-owned **pure projection** as `List`/`Tabs`/`Gauge`, with the
  out-of-range `position` clamped (never panicking). No framing `Block` (it is
  a one-cell edge adornment, not a container) and — every part being one
  `char` — the first widget with no lifetime parameter.
- `spinner` — `Spinner`: a one-cell animated busy indicator
  (`BRAILLE`/`LINE`/`DOTS`/`ARC` sets, or your own `char` frames). The same
  caller-owned **pure projection** as the others, on the *time* axis: it shows
  `frames[tick % frames.len()]` and only reads the caller-owned `tick`
  (typically `frame.count()`) — the first consumer of the `Frame::count()`
  animation clock. An empty frame set is a total no-op, never a modulo panic.
- `table` — `Table`/`Row`: a column-aligned grid, the 2D generalization of
  `List`. Columns are sized by the same `Constraint` divider top-level
  `Layout` uses (with `column_spacing`); an optional fixed `header` labels
  them, and the selected data row gets the same full-width highlight bar and
  reserved gutter as `List`. The same caller-owned **pure projection** —
  `selected`/`offset` are read, never render-mutated — and **total**: an
  out-of-range width percentage is clamped where ratatui panics.
- `checkbox` — `Checkbox`: a single-line labelled boolean control
  (`[x] Enable logging`), the first of the interactive form-control family
  and the first widget to model a **focus** visual. A **pure projection** of
  *two* caller-owned `bool`s — `checked` (the data, like `List`'s `selected`)
  and `focused` (drawn with a `focus_style` patched last, the same
  highlight-wins-last bar `List` uses for selection). It renders a focused
  control but deliberately does **not** decide *which* control is focused:
  focus *routing* was kept out of the widget as a separate,
  expensive-to-reverse decision, now resolved in
  [ADR 0004](docs/adr/0004-focus-routing-architecture.md) (focus is
  caller-owned model state the pure `view` projects in — the widget's
  `focused: bool` is exactly that contract). A leaf control like
  `Scrollbar`/`Spinner` — no framing
  `Block`, one row — and **total** (narrow/empty/multi-row areas clip safely).
- `button` — `Button`: a single-line **centred** focusable *action* label,
  the second form control and the first with **no data state at all** — a
  **pure projection** of only a caller-owned `focused` `bool` (drawn with the
  same `focus_style`-patched-last full-width bar as `Checkbox`). What a press
  *does* is the reducer's concern in `update`; the widget only renders the
  affordance. The label is centred by default, but the label `Line`'s own
  alignment wins (the line-wins-over-container rule a `Block` title uses). A
  leaf control like `Checkbox` — no framing `Block`, one row — and **total**.
- `radio` — `Radio`: a single-line labelled **exclusive-choice** control
  (`(•) High`), the third form control and the exclusive-selection sibling
  of `Checkbox`. A **pure projection** of *two* caller-owned `bool`s —
  `selected` (the data, the `List`-style "which one is chosen" concept, so
  not `Checkbox`'s independent `checked`) and `focused` (the same
  `focus_style`-patched-last full-width bar the other form controls use).
  Exactly-one-per-group is the **caller's invariant**, not the widget's: the
  model holds one chosen index and projects `selected(i == chosen)` per
  option (gpui-component-validated — its `Radio` says the group "is not
  included … you can manage the group by yourself"). A `RadioGroup`
  convenience (one owned index + layout) is a deliberately deferred
  *additive* future widget. A leaf control like `Checkbox` — no framing
  `Block`, one row — and **total** (narrow/empty/multi-row areas clip safely).
- `input` — `Input`: a single-line **text-entry** field, the fourth form
  control, the **first text-edit/cursor widget**, and the first
  [`focus`](docs/adr/0004-focus-routing-architecture.md) consumer. A **pure
  projection** of a *borrowed* caller-owned `TextEdit` (the rstui-core
  `String`+character-cursor model) plus `focused`. The widget cannot reach the
  `Frame`, so it draws its **own** caret (default reverse-video at the cursor
  column) rather than the terminal hardware cursor — the only TTY-free
  snapshot-testable choice, and the OpenTUI-aligned one. The caret-following
  horizontal scroll is a **stateless pure function of cursor + width** (no
  caller-owned scroll state; a `List`-style owned `offset` is a deferred
  additive). `focus_style` is the same patched-last full-width bar the other
  form controls use; an optional `placeholder` shows while empty. A leaf
  control like `Checkbox` — no framing `Block`, one row — and **total**
  (one-cell/empty/multi-byte/multi-row areas clip safely). Driven end to end
  across two fields via `Harness` in the `input_demo` example.
- `modal` — `Modal`: a centred, **opaque**, optionally-`Block`-framed dialog
  over an overlay area — the *visual* half of the modal-focus model whose
  *state* half is the `FocusRing` scope stack
  ([ADR 0004](docs/adr/0004-focus-routing-architecture.md) §6, **Follow-up §3**).
  A **pure projection**: the widget never reads focus; the app decides "a
  modal is open" (`ring.in_scope()`) and `view` renders it. It **clears** its
  box via `Buffer::clear_region` so background content cannot bleed through
  (the defining always-on affordance — the `Input`-caret precedent); the
  optional `backdrop_style` scrim is opt-in. Sizing reuses the `Constraint`
  vocabulary (like `Table`'s columns), centred; `inner()`/`area()` are pure
  derived rects. The `modal_demo` example wires both halves under `Harness`,
  proving declarative trapping, scope-constrained `Tab`, and validated
  capture/restore TTY-free.
- `status_bar` — `StatusBar`: a one-row strip with independently
  left-/centre-/right-anchored `Line` segments over a base-style fill — the
  editor/file-manager status strip (mode + path, a transient message, cursor
  position). The first **multi-anchor** layout widget and a **pure
  projection** of three caller-built segments. Contention is resolved by one
  fixed, documented rule (right is anchored and kept intact; left is clipped
  before it reaches right; centre draws only in the gap between them), so the
  output is always well-defined and **total** under any width. A leaf control
  like `Input` — no framing `Block`; the surrounding `Layout` owns the edge it
  pins to. The `status_bar_demo` example renders the canonical bottom strip
  TTY-free.
- `toast` — `Toast`: a corner-anchored, **opaque** stack of transient
  `ToastMessage` notifications (the editor "saved"/"build failed" strip), the
  first **floating multi-box** widget. A **pure projection** of a caller-owned
  `&[ToastMessage]` — `messages[0]` is the newest, anchored flush to a
  `ToastCorner`; older entries stack away from it across `gap` blank rows, only
  `max_visible` drawn — so *expiry and dismissal stay the reducer's job*, never
  a wall clock smuggled into the pure `view` (the `Spinner` caller-owned-tick
  precedent). Each box `clear_region`s itself opaque (the `Modal` affordance)
  and soft-wraps its body by **reusing `Paragraph`** (`Paragraph::line_count`
  sizes the box — no second wrap algorithm), with per-`ToastLevel` accent
  styles and an optional framing `Block`. **Total** (empty list/overlay,
  `max_visible == 0`, over-wide/over-tall bodies all clip safely). The
  `toast_demo` example renders a realistic info/success/warning/error stack
  over background content TTY-free.

### `rstui-runtime`

The Elm/Bubble Tea–style application loop, expressed as a contract so the same
app code runs headless today and under a real terminal later.

- `App` — the trait you implement on your state: `init`/`on_event`/`update`/
  `view`. State changes flow through `update` only; `on_event` (taking
  `&self`) maps input to intent and `view` (taking `&self`) just renders.
- `Cmd` — the side effects an `update` schedules (`none`, `quit`, `message`,
  `perform`, `batch`). The runtime performs them and feeds resulting messages
  back into `update`; the app never does IO or quits directly.
- `Harness` — a deterministic, terminal-free driver that runs the real loop
  over `rstui-core`'s `TestBackend`, so whole apps are unit-testable (assert
  on state and the rendered snapshot) with no TTY, threads, or clock.
- `run` — the **live** event loop, generic over any `rstui-core` `Backend` +
  `EventSource`. It shares one `settle` command-settling core with `Harness`,
  so the harness is not merely *a* reference for the live loop — it is
  literally the same reducer logic with a `TestBackend` and scripted input
  swapped in. The same `App`/`Cmd` code runs headless in tests and live on a
  terminal, unchanged. Commands run inline and the loop blocks on input
  (threaded commands and a periodic tick are separable future slices,
  deliberately deferred, not stubbed).

### `rstui-crossterm`

The crossterm-backed terminal driver, and the deliberate home of the
workspace's only external dependency so `rstui-core` stays dependency-free
(see [ADR 0001](docs/adr/0001-terminal-backend-strategy.md)). Apps never
import crossterm; they depend on `rstui-core`'s `Backend` trait and `Event`
vocabulary, so the backend stays swappable.

- `from_crossterm` — a pure, total, terminal-free translation from a
  `crossterm::event::Event` to an `rstui-core` `Event`. Because rstui shaped
  its core event model 1:1 like crossterm's on purpose, the map is
  near-mechanical and fully unit-testable with hand-built events and no TTY
  (ADR 0001 testing layer L4a). Input rstui deliberately does not model
  (Kitty-only lock/media/modifier keys, the `HYPER`/`META` modifiers, lock
  state) is dropped rather than stubbed, matching the core's "defer, do not
  stub" discipline.
- `CrosstermBackend<W: io::Write>` — the `rstui-core` `Backend` impl. Every
  escape is *queued* (never `execute!`d) and flushed once per frame, a
  deliberate divergence from ratatui made possible because rstui's `Terminal`
  owns the loop; an empty diff emits zero bytes. The cell diff drives the
  proven minimal-output algorithm (cursor moved only on a discontinuity,
  colors/attributes re-emitted only on change). Generic over any writer, so
  the full ANSI output is asserted in-memory with no TTY (ADR 0001 testing
  layer L4b); only `size`/`cursor_position` query the real terminal (L4c).
- `TerminalGuard` / `LifecycleOptions` — a panic-safe RAII guard that
  enables the requested terminal modes (raw mode, alternate screen,
  mouse/paste/focus reporting) and restores exactly those on drop,
  **including while unwinding from a panic**. It wraps `CrosstermBackend`
  and is itself a `Backend`, so it drops into `Terminal` for one
  panic-safe ownership chain — a deliberate divergence from ratatui's
  free `init`/`restore`, affordable because rstui owns the loop. The
  enter/leave choreography is asserted in memory with no TTY (raw mode is
  the only PTY-only seam, gated by `LifecycleOptions::raw_mode`).
- `CrosstermEventSource` — the `rstui-core` `EventSource` impl, folding
  crossterm's `poll`/`read` into one timed call and translating via
  `from_crossterm`. Blocking mode **skips** unmodeled input (a CapsLock press
  is ignored, never read as the end-of-input that would stop the app); timed
  mode does one poll and at most one read so an animation tick is never
  starved. Generic over a private reader seam exactly as `CrosstermBackend`
  is generic over `io::Write`, so every decision branch is asserted in memory
  with no TTY; only the real reader's two `crossterm::event::{poll, read}`
  calls are the PTY-only surface (ADR 0001 testing layer L4c).
- With this, the framework composes end to end. The `run_app` example wires
  `TerminalGuard` + `CrosstermBackend` + `CrosstermEventSource` into
  `rstui_runtime::run` to drive an unmodified `App` on a real terminal — the
  *same* `run` the headless harness tests call over a `TestBackend` +
  `TestEventSource`.
- Next: a feature-gated async `EventStream` source, and an opt-in panic hook
  so a panicking app's message stays visible (a concern separate from
  teardown, belonging with the runtime driver).

## Architecture decisions

Decisions that are expensive to reverse are recorded as dated, immutable
Architecture Decision Records in [`docs/adr`](docs/adr). They capture the
context, the options weighed, the decision, and the evidence behind it.

- [ADR 0001 — Terminal backend strategy](docs/adr/0001-terminal-backend-strategy.md):
  crossterm behind a dedicated `rstui-crossterm` crate is the default
  backend; `rstui-core` keeps owning the `Backend` trait and `TestBackend`;
  the trait stays the single seam so an optional high-fidelity `rstui-termwiz`
  crate remains possible later. Also fixes the four-layer end-to-end testing
  contract every new capability must satisfy.
- [ADR 0002 — Widget crate boundary](docs/adr/0002-widget-crate-boundary.md):
  concrete widgets move out of `rstui-core` into a single grouped
  `rstui-widgets` crate (one module per widget, **not** one crate per
  widget); `rstui-core` keeps the `Widget` trait and the primitives; the
  bounds-safe cell-stamping helper becomes a public `Buffer` method so
  third-party widget crates have the same authoring contract; a widget is
  feature-gated only when it adds a transitive dependency; an umbrella
  `rstui` crate is deferred until a second backend or a feature-gated
  widget exists.
- [ADR 0003 — Lint and code-quality policy](docs/adr/0003-lint-and-code-quality-policy.md):
  an evidence-shaped tiered policy rolled out in reviewable phases.
  Clippy default groups stay denied in CI; `clippy::pedantic` is
  opt-in at `warn` with an explicit allow-list and lands as its own
  slice; `nursery`/`cargo`/`restriction` groups are never adopted
  wholesale; `unsafe_code`/`missing_docs` consolidate into
  `[workspace.lints.*]`; a `cargo doc` `-D warnings` gate closes the
  rustdoc silent-failure gap. Phase 3's vague-generic-naming check has
  landed as the first rstui-specific lint (`cargo xtask lint-names`,
  [convention](docs/conventions/naming.md)); supply-chain
  (`cargo-deny`, `cargo-machete`) and an MSRV CI leg remain sequenced
  as later independent slices.
- [ADR 0004 — Focus-routing architecture](docs/adr/0004-focus-routing-architecture.md):
  focus is **caller-owned model state**, mutated only in `update` and
  read by the pure `view` to project `focused: bool` into widgets —
  the only shape compatible with rstui's pure-view, immediate-mode,
  single-testable-reducer invariants (runtime- and widget-owned focus
  and retained-tree traversal are rejected). The zero-framework floor
  (an app's own `enum` + `focused: bool`, the existing
  `Checkbox`/`Button`/`Radio` contract) is permanent; the optional,
  pure, model-resident `rstui_core::focus` primitive (`FocusId` +
  a total, wrapping `FocusRing`) that reduces the boilerplate has
  landed (Follow-up §1), as has its editing-side dual
  `rstui_core::text_edit::TextEdit` and the `rstui-widgets` `Input`
  widget that projects a borrowed `TextEdit` + `focused` (the first
  `FocusRing` consumer), driven across two fields via `Harness` in
  the `input_demo` example — **Follow-up §2 is complete**. The §6
  modal model has landed in `rstui_core::focus` as the `FocusRing`
  scope stack (`push_scope`/`pop_scope`, scope-constrained traversal,
  captured/validate-restored focus, `in_scope`/`scope_depth` for
  declarative reducer-gated trapping), and the `rstui-widgets` `Modal`
  widget — the centred opaque dialog that projects it — with the
  `modal_demo` example wiring both halves under `Harness` (declarative
  trapping, scope-constrained `Tab`, validated capture/restore, all
  TTY-free) — **Follow-up §3 is complete**. Terminal
  `FocusGained`/`FocusLost` stay distinct from widget focus.

## Build & test

```sh
cargo test --all-features
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo xtask lint-names
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --workspace
cargo run -p rstui-core --example buffer_demo
cargo run -p rstui-core --example terminal_loop
cargo run -p rstui-widgets --example block_demo
cargo run -p rstui-widgets --example text_demo
cargo run -p rstui-widgets --example paragraph_demo
cargo run -p rstui-widgets --example list_demo
cargo run -p rstui-widgets --example tabs_demo
cargo run -p rstui-widgets --example gauge_demo
cargo run -p rstui-widgets --example scrollbar_demo
cargo run -p rstui-widgets --example spinner_demo
cargo run -p rstui-widgets --example table_demo
cargo run -p rstui-widgets --example checkbox_demo
cargo run -p rstui-widgets --example button_demo
cargo run -p rstui-widgets --example radio_demo
cargo run -p rstui-widgets --example input_demo
cargo run -p rstui-widgets --example modal_demo
cargo run -p rstui-widgets --example status_bar_demo
cargo run -p rstui-widgets --example toast_demo
cargo run -p rstui-runtime --example counter
```

CI runs the same fmt, clippy, naming, doc (`-D warnings`), and test gates
on every push and pull request. Lint policy lives in one place — the
`[workspace.lints.*]` tables in the root `Cargo.toml` — per
[ADR 0003](docs/adr/0003-lint-and-code-quality-policy.md); the
project-specific naming rule is in
[`docs/conventions`](docs/conventions/naming.md).

## GNHF Claude Runner

Use the repo-local helper to run gnhf with Claude Code on Opus 4.7 using max
effort.

```sh
npm install -g gnhf
claude setup-token

scripts/run-gnhf-rstui.sh
```

By default this runs:

```sh
--current-branch --push
```

That keeps going until you stop it, gnhf reaches its failure limit, or you pass
your own stop condition. Pass custom gnhf flags to override the defaults:

```sh
scripts/run-gnhf-rstui.sh --worktree --max-iterations 10
```

Set `RSTUI_CLAUDE_MODEL` or `RSTUI_CLAUDE_EFFORT` to override the Claude model
or effort level for the wrapper.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
