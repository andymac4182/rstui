# rstui

An idiomatic Rust TUI framework for building powerful terminal applications
quickly.

rstui learns from the architecture of OpenTUI, the real application needs of
OpenCode, the update/view/event-loop ergonomics of Bubble Tea, the terminal
rendering ecosystem of ratatui, and the breadth and polish of gpui-component —
while staying idiomatic to Rust.

> **Status:** early foundation. The rendering substrate and the terminal
> `Backend` boundary exist and are tested; the runtime, components, and plugin
> host are not built yet.

## Workspace

The project is a Cargo workspace (Rust 2024 edition). Crates are introduced
only when there is enough real API surface to justify the boundary.

| Crate                | Responsibility                                                       |
| -------------------- | -------------------------------------------------------------------- |
| `crates/rstui-core`  | Dependency-free rendering substrate: geometry, style, buffer, backend |

Planned boundaries as the framework grows: a core runtime (Elm-style
model/update/view loop), layout, a component set, examples, and a permissioned
plugin host built on process isolation.

### `rstui-core`

`rstui-core` is pure and deterministic — no terminal, async runtime, or event
loop — so every layer above it can be unit tested without a TTY.

- `geometry` — `Position`, `Size`, `Rect`, `Margin`. `Rect::new` clamps so
  edge accessors can never overflow.
- `style` — `Color`, `Modifier`, and a composable `Style` patch model that
  themes, focus, and selection highlights build on.
- `buffer` — `Cell` and the immediate-mode `Buffer` grid widgets draw into and
  renderers `diff` against.
- `backend` — the `Backend` trait (the screen boundary that consumes a
  `Buffer` diff) and an in-memory `TestBackend` so UIs are testable without a
  TTY. Real terminal backends will live in their own crate.

## Build & test

```sh
cargo test --all-features
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo run -p rstui-core --example buffer_demo
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
