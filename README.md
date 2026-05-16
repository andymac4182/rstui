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
> sub-cell-precision `Gauge` progress bar,
> the styled-text
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
| `crates/rstui-core`      | Dependency-free substrate: geometry, style, stylize, layout, buffer, backend, terminal, event, event_source, the `Widget` trait, text |
| `crates/rstui-widgets`   | The concrete widget set ([ADR 0002](docs/adr/0002-widget-crate-boundary.md)), one module per widget — `Block`, `Paragraph`, `List`, `Tabs`, and `Gauge` today. Depends only on `rstui-core`; the worked reference for third-party widget crates |
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
— `Block`, `Paragraph`, `List`, `Tabs`, and `Gauge` ship there today, with
`Buffer::set_cell`
the public cell-stamping contract third-party widgets build on; `Alignment`
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
  renderers `diff` against.
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
