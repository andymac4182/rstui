# rstui documentation

An idiomatic Rust TUI framework for building powerful terminal applications
quickly. This is the documentation home — start here and follow the path that
fits what you are doing.

## Read in this order

| # | Doc | What it gives you |
|---|-----|-------------------|
| 1 | [Getting started](getting-started.md) | Install, the smallest app, how to run every example and the kitchen sink |
| 2 | [Architecture](architecture.md) | The one mental model (immediate-mode, pure projection, caller-owned state), the crate map, the ADR index |
| 3 | [Core reference](core-reference.md) | `rstui-core`: geometry, style, layout, buffer, terminal, events, focus, the text + editing models |
| 4 | [Runtime](runtime.md) | The Elm-style `App`/`Cmd` contract, the `Harness`, the live `run` loop, the crossterm wiring |
| 5 | [Component library](widgets/README.md) | Every widget, grouped by family, with API, state model, a runnable demo and a recorded GIF |
| 6 | [Plugin system](plugins.md) | The deny-by-default permissioned plugin host, end to end |
| 7 | [ACP client](acp-client.md) | The Agent Client Protocol chat client built on the framework |
| 8 | [Testing](testing.md) | Deterministic, TTY-free testing with `Harness` + `TestBackend`, and VHS golden e2e |
| 9 | [Kitchen sink](kitchen-sink.md) | The full-screen showcase app and its multi-resolution recordings |
| 10 | [Recording](recording.md) | How the GIFs/videos are produced and regenerated with VHS |

## Quick links

- **Architecture decisions** — [`docs/adr/`](adr/README.md) (12 ADRs; the *why* behind every boundary)
- **Composition model** — [`docs/composition.md`](composition.md) (how widgets compose into a screen; ADR 0012)
- **Theming** — [`docs/theming.md`](theming.md) (every gpui-component theme, as a terminal palette)
- **Conventions** — [`docs/conventions/`](conventions/README.md) (naming gate)
- **Inner loop** — [`docs/development.md`](development.md) (`cargo xtask ci`)
- **Merging** — [`docs/merging.md`](merging.md) (parallel-stream merge protocol)
- **Benchmarking** — [`docs/benchmarking.md`](benchmarking.md) (the non-gating slow loop)
- **Keeping docs current** — the [`rstui-docs` skill](../.claude/skills/rstui-docs/SKILL.md)

## The workspace at a glance

| Crate | Responsibility |
|-------|----------------|
| [`rstui-core`](core-reference.md) | Dependency-free substrate: geometry, style, layout, buffer, backend, terminal, event, focus, the `Widget` trait, text, text-edit, text-area, scroll, selection |
| [`rstui-widgets`](widgets/README.md) | The concrete widget set (~57), one module per widget. The worked reference for third-party widget crates |
| [`rstui-runtime`](runtime.md) | Elm-style `App`/`Cmd`, the deterministic `Harness`, and the live `run` loop they share |
| [`rstui-crossterm`](runtime.md#crossterm-the-live-terminal) | The crossterm-backed terminal driver — the only external dependency, isolated here |
| [`rstui-plugin-host`](plugins.md) | Dependency-free permissioned plugin host: plugins run as separate OS processes, deny-by-default |
| [`rstui-acp-client`](acp-client.md) | A full-screen Agent Client Protocol chat client built on the framework |
| [`rstui-theme`](theming.md) | Optional: every gpui-component theme as a terminal-ready palette + `Style` constructors |
| `rstui-kitchen-sink` | The interactive full-screen showcase exercising every widget — see [Kitchen sink](kitchen-sink.md) |
| `rstui-bench` / `xtask` | The non-gating benchmark harness and workspace automation (`cargo xtask ci`, `record`) |

## One model, stated once

Everything below rests on a single idea, documented in
[Architecture](architecture.md) and proven by the `gallery` example:

> The **model owns the state**. `view` is a **pure projection** of that state
> into a frame — widgets only read, never mutate, never panic. `update` is the
> **only** place state changes. The same `App` runs deterministically in tests
> (`Harness`) and live on a real terminal (`run`), unchanged.

If you remember one thing, remember that. The rest is vocabulary.
