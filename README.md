# rstui

An idiomatic Rust TUI framework for building powerful terminal applications
quickly.

rstui learns from the architecture of OpenTUI, the real application needs of
OpenCode, the update/view/event-loop ergonomics of Bubble Tea, the terminal
rendering ecosystem of ratatui, and the breadth and polish of gpui-component —
while staying idiomatic to Rust.

> **Status:** early foundation. The rendering substrate, the terminal
> `Backend` boundary, the double-buffered `Terminal` frame driver, the
> constraint-based `Layout` divider, the keyboard/mouse/focus/resize `Event`
> model, the `Widget` abstraction with the foundational `Block` container, and
> the Elm-style `App`/`Cmd`/`Harness` runtime exist and are tested; a real
> terminal driver, a broader component set, and the plugin host are not built
> yet.

## Workspace

The project is a Cargo workspace (Rust 2024 edition). Crates are introduced
only when there is enough real API surface to justify the boundary.

| Crate                  | Responsibility                                                                 |
| ---------------------- | ------------------------------------------------------------------------------ |
| `crates/rstui-core`    | Dependency-free substrate: geometry, style, layout, buffer, backend, terminal, event, widget |
| `crates/rstui-runtime` | Elm-style `App`/`Cmd` contract and a deterministic terminal-free test harness  |

Planned boundaries as the framework grows: a real terminal driver, a broader
component set (the `Widget` trait lives in core; concrete widgets beyond
`Block` will graduate to their own crate once there are enough to justify it),
more examples, and a permissioned plugin host built on process isolation.

### `rstui-core`

`rstui-core` is pure and deterministic — no terminal, async runtime, or event
loop — so every layer above it can be unit tested without a TTY.

- `geometry` — `Position`, `Size`, `Rect`, `Margin`. `Rect::new` clamps so
  edge accessors can never overflow.
- `style` — `Color`, `Modifier`, and a composable `Style` patch model that
  themes, focus, and selection highlights build on.
- `layout` — `Layout`, `Direction`, and the `Constraint` vocabulary
  (`Length`/`Percentage`/`Ratio`/`Min`/`Max`/`Fill`) for dividing a `Rect`
  into contiguous regions. A deterministic, integer-only divider (no Cassowary
  solver, no floats) that always tiles the area exactly.
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
- `widget` — the `Widget` trait (`render(self, area, buf)`) every component
  implements, blanket impls for `&str`/`String`/`Option<W>`, and the
  foundational `Block` container: `Borders`, `BorderType`, `Padding`, a styled
  fill, and a clipped aligned title, with `Block::inner` handing the remaining
  area to the content drawn inside. `Frame::render_widget` is the entry point.

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
  on state and the rendered snapshot) with no TTY, threads, or clock. It is
  the reference semantics for the future real runtime.

## Build & test

```sh
cargo test --all-features
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo run -p rstui-core --example buffer_demo
cargo run -p rstui-core --example terminal_loop
cargo run -p rstui-core --example block_demo
cargo run -p rstui-runtime --example counter
```

CI runs the same fmt, clippy, and test gates on every push and pull request.

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
