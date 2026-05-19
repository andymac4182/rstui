# Architecture

rstui has exactly one mental model. Learn it once and every crate, widget and
test reads the same way.

## The model

> **Immediate-mode, pure projection, caller-owned state.**
>
> - The **model** (your `App`) owns all state.
> - **`view`** is a *pure projection*: it builds throwaway widgets from the
>   model and stamps them into a `Buffer`. Widgets only **read**. They never
>   retain, never mutate, never panic.
> - **`update`** is the *only* place state changes. It folds one message into
>   the model and returns a `Cmd` describing any side effects.
> - There is **no retained widget tree**. Widgets are values built fresh each
>   frame and dropped. Composition is `Layout` splitting a `Rect` plus `Rect`
>   accessors, not parent/child objects.

This is recorded as [ADR 0012](adr/0012-widget-composition-and-layout-model.md)
and walked through, with code, in [`docs/composition.md`](composition.md). The
`gallery` example is its executable proof:

```sh
cargo run  -p rstui-widgets --example gallery   # every rstui-widgets widget, one reducer
cargo test -p rstui-widgets --example gallery   # the same, asserted
```

### Why "pure projection" matters

Three properties fall out of it for free:

1. **Determinism.** `view(model)` always produces the same frame for the same
   model. That is why a test can assert on a string snapshot.
2. **Totality.** Every widget and every geometry/buffer op is *total*: arbitrary
   input (zero-area rects, out-of-range indices, oversized text) renders
   sensibly — clamped, clipped or a no-op — and **never panics**. This is the
   "iter-25 rule" referenced throughout the codebase.
3. **One app, two environments.** Because the reducer never touches the
   terminal, the *same* `App` runs under the deterministic `Harness` (a
   `TestBackend`, no threads, no clock) and under the live `run` loop (a real
   terminal) with **no changes**. Tests are not a copy of the app; they *are*
   the app.

### Caller-owned state, not framework-owned

Focus, scroll position, text being edited, selection — all live in *your*
model as plain value types (`FocusRing`, `ScrollState`, `TextEdit`,
`TextArea`, `Selection`). Widgets receive them by reference and *project* them.
The framework never owns a focus tree or a scroll register behind your back
([ADR 0004](adr/0004-focus-routing-architecture.md)). `update` steps these
models; `view` reads them.

## The crate map

The workspace introduces a crate boundary only when there is enough real API
surface to justify it.

```
                 rstui-core         (zero deps; the substrate + the Widget trait)
                /     |      \
   rstui-widgets  rstui-runtime  rstui-plugin-host   (each depends only on core)
        |              |                 (zero deps, no unsafe)
   rstui-code     rstui-crossterm
  (core+widgets+   (the only crate with an external dep: crossterm)
   tree-sitter;          |
   the only        rstui-acp-client     (a real app: ACP chat, plugins in the TUI)
   tree-sitter
   consumer — ADR 0024)
```

| Crate | Owns | Depends on | Doc |
|-------|------|-----------|-----|
| `rstui-core` | Geometry, style, layout, buffer, terminal, event, focus, the `Widget` trait, text, the editing/scroll/selection models | *nothing* | [Core reference](core-reference.md) |
| `rstui-widgets` | The ~72 concrete general widgets, one module each; tree-sitter-free | `rstui-core` | [Component library](widgets/README.md) |
| `rstui-code` | The code-editing widgets (`Editor`, `Diff`, `LineNumberGutter`) + the syntax/outline/changeset models + the first-class tree-sitter engine; the only crate that pulls tree-sitter (ADR 0024) | `rstui-core`, `rstui-widgets`, `tree-sitter` | [Code widgets](widgets/code.md) |
| `rstui-runtime` | `App`/`Cmd`, the `Harness`, the live `run` loop | `rstui-core` | [Runtime](runtime.md) |
| `rstui-crossterm` | crossterm `Backend` + `EventSource` + panic-safe lifecycle, `run_app` | `rstui-core`, `rstui-runtime`, crossterm | [Runtime](runtime.md#crossterm-the-live-terminal) |
| `rstui-plugin-host` | Capability model, manifest, policy, frame protocol, host mediation, plugin SDK | *nothing*, no `unsafe` | [Plugin system](plugins.md) |
| `rstui-acp-client` | A full-screen ACP chat client with a TUI plugin layer | the above | [ACP client](acp-client.md) |
| `rstui-ai` | The AI-app widget set: the AI-SDK message model, a streaming-markdown view, the ai-elements vocabulary | `rstui-core`, `rstui-widgets` | [Agent UI](agent-ui.md) |
| `rstui-jsonui` | The declarative agent-UI engine: parses A2UI + json-render, projects them to one `UiNode` tree | core, widgets, `rstui-ai` | [Agent UI](agent-ui.md) |
| `rstui-git-review` | A full-screen git history review + code editing app (git is a `Cmd`-seam subprocess) | core, runtime, crossterm, widgets | [git review](git-review.md) |
| `rstui-kitchen-sink` | The interactive showcase of every widget | the above | [Kitchen sink](kitchen-sink.md) |
| `rstui-bench` / `xtask` | Non-gating benchmarks; workspace automation | — | [development.md](development.md) / [benchmarking.md](benchmarking.md) |

`rstui-core` being dependency-free is the keystone: it lets *every* higher
layer be tested without a TTY, an async runtime or a clock
([ADR 0002](adr/0002-widget-crate-boundary.md)).

## The data flow, one frame

```
            ┌──────────────── your App (the model) ────────────────┐
            │                                                       │
 input ──▶ on_event(&self, Event) ──▶ Option<Msg>                   │  reads only
            │                            │                          │
            │                            ▼                          │
 tick  ──▶ on_tick(&self) ─────────▶ update(&mut self, Msg) ─▶ Cmd  │  the only mutation
            │                            │   ▲                       │
            │                            ▼   │ (settle: Cmd messages │
            │                          effects │  fold back in)      │
            │                                                        │
            └──────────▶ view(&self, &mut Frame) ────────────────────┘  pure projection
                              │
                              ▼
                  Buffer  ──diff──▶  Backend  ──▶  terminal / TestBackend
```

`on_event` and `on_tick` take `&self` on purpose — deciding what input *means*
may depend on state but must not change it. All mutation flows through
`update`. The runtime *settles* commands to a fixed point (a `Cmd` message
re-enters `update`) and then renders by diffing the new `Buffer` against the
previous one, sending only changed cells. The full contract is in
[Runtime](runtime.md).

## Architecture decision records

Every boundary above is a recorded decision. The ADRs are the authoritative
*why*; this page is the *what*. Skim the index, read the ones relevant to what
you are changing.

| ADR | Decision |
|-----|----------|
| [0001](adr/0001-terminal-backend-strategy.md) | crossterm as the terminal backend, isolated in one crate |
| [0002](adr/0002-widget-crate-boundary.md) | `rstui-widgets` split from a dependency-free `rstui-core` |
| [0003](adr/0003-lint-and-code-quality-policy.md) | Centralized lint policy; the gates; `unsafe_code = forbid` |
| [0004](adr/0004-focus-routing-architecture.md) | Focus is caller-owned model state with a modal scope stack |
| [0005](adr/0005-benchmarking-and-profiling-strategy.md) | Non-gating, dependency-free, deterministic benchmark harness |
| [0006](adr/0006-runtime-tick-and-loop-model.md) | The Elm-style tick/loop model (`tick_rate`/`on_tick`) |
| [0007](adr/0007-plugin-host-and-secure-execution.md) | The deny-by-default permissioned plugin host |
| [0008](adr/0008-async-command-executor.md) | The off-loop command executor seam (stdlib threads) |
| [0009](adr/0009-optional-async-runtime-policy.md) | Optional async-runtime policy (superseded by 0011) |
| [0010](adr/0010-production-ci-and-release-readiness-posture.md) | CI + release-readiness posture (MSRV, supply chain) |
| [0011](adr/0011-async-event-loop.md) | The async event loop (`run_async`, `tokio::select!`) |
| [0012](adr/0012-widget-composition-and-layout-model.md) | The immediate-mode pure-projection composition model |
| [0013](adr/0013-terminal-emulator-compatibility.md) | Terminal-emulator compatibility & control-code posture |
| [0014](adr/0014-comprehensive-interactive-datatable.md) | Comprehensive interactive DataTable: reducer-run pipeline, pure projection, borrowed-`TextEdit` editing |
| [0015](adr/0015-keymap-architecture.md) | Customisable keymap engine as a shared crate (`rstui-keymap`) |
| [0016](adr/0016-shared-memory-plugin-transport.md) | Shared-memory plugin transport (opt-in, Rust-only, spin) |
| [0017](adr/0017-ai-app-widgets-and-declarative-agent-ui.md) | AI-app widgets + declarative agent-driven UI rendering (`rstui-ai`, `rstui-jsonui`) |
| [0022](adr/0022-syntax-colour-and-symbol-engine.md) | Syntax-colour & symbol engine: dependency-free lexer floor + optional feature-gated tree-sitter tier (one parse → highlight *and* symbols) |
| [0023](adr/0023-treesitter-tier1-excluded-leaf-crate.md) | tree-sitter Tier-1 as a workspace-`exclude`d opt-in leaf crate — *superseded by [0024](adr/0024-code-widget-crate-and-treesitter-exemption.md)* |
| [0024](adr/0024-code-widget-crate-and-treesitter-exemption.md) | `rstui-code` widget crate (move `Editor`/`Diff`/code modules + fold the tree-sitter engine in) with a first-class, gate-protected tree-sitter exemption — supersedes 0023, amends 0022 & 0002; only `rstui-code` consumers pull tree-sitter |
| [0026](adr/0026-calendar-app-widget-suite.md) | Calendar-app widget suite — one shared caller-owned `CalendarEvent` model (+ `pack_day` overlap packer) and nine pure-projection views/controls (Month/Week/Day/Agenda/Year, TimePicker, EventCard, EventEditor, DateNavigator); no date math (ADR 0002 §4), the app owns move/schedule via hit accessors (ADR 0012); purely additive |

This table is a curated subset; see [`docs/adr/README.md`](adr/README.md) for
the complete ADR index, format and statuses.
