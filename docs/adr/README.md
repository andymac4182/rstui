# Architecture Decision Records

This directory records the architectural decisions that shape rstui:
the choices that are expensive to reverse, constrain later work, or
would otherwise have to be re-derived from scratch each time someone
asks "why is it built this way?".

An ADR is a short, dated, immutable document. It captures the context
that forced a decision, the options considered, the decision taken, and
the consequences accepted. Decisions are not edited after the fact — if
one is revisited, a new ADR supersedes it and links back.

## Format

Each record is `NNNN-kebab-title.md` and follows a lightweight
MADR-style shape:

- **Status** — Proposed, Accepted, Superseded by `NNNN`.
- **Context** — the forces in play, including constraints already
  locked in by earlier slices.
- **Decision drivers** — the axes the decision is judged on.
- **Options considered** — each with its real tradeoffs.
- **Decision** — the choice, stated plainly, with rationale.
- **Evidence** — concrete facts from the reference projects, cited, so
  the reasoning can be audited rather than taken on faith.
- **Consequences** — what this makes easy, what it makes hard, and what
  is deliberately deferred.

## Index

| ADR | Title | Status |
| --- | ----- | ------ |
| [0001](0001-terminal-backend-strategy.md) | Terminal backend strategy | Accepted |
| [0002](0002-widget-crate-boundary.md) | Widget crate boundary | Accepted |
| [0003](0003-lint-and-code-quality-policy.md) | Lint and code-quality policy | Accepted |
| [0004](0004-focus-routing-architecture.md) | Focus-routing architecture | Accepted |
| [0005](0005-benchmarking-and-profiling-strategy.md) | Benchmarking and profiling strategy | Accepted |
| [0006](0006-runtime-tick-and-loop-model.md) | Runtime tick and loop model | Accepted |
| [0007](0007-plugin-host-and-secure-execution.md) | Plugin host and secure execution | Accepted |
| [0008](0008-async-command-executor.md) | Off-loop command executor (threads, no async dependency) | Accepted |
| [0009](0009-optional-async-runtime-policy.md) | Optional async-runtime policy (feature-gated tokio) | Superseded by [0011](0011-async-event-loop.md) |
| [0010](0010-production-ci-and-release-readiness-posture.md) | Production CI and release-readiness posture | Accepted |
| [0011](0011-async-event-loop.md) | Async event loop (`tokio::select!`) | Accepted |
| [0012](0012-widget-composition-and-layout-model.md) | Widget composition and layout model (immediate-mode, pure projection) | Accepted |
| [0013](0013-terminal-emulator-compatibility.md) | Terminal-emulator compatibility & control-code posture | Accepted |
| [0014](0014-comprehensive-interactive-datatable.md) | Comprehensive interactive DataTable (reducer-run pipeline, pure projection, borrowed-`TextEdit` editing) | Accepted |
| [0015](0015-keymap-architecture.md) | Customisable keymap engine as a shared crate (semantic actions, per-OS, multi-keymap, leader sequences, runtime remap; Textual + OpenCode synthesis) | Accepted |
| [0016](0016-shared-memory-plugin-transport.md) | Shared-memory plugin transport (opt-in, Rust-only, scoped spin — flat sub-µs RTT) | Accepted |
| [0017](0017-ai-app-widgets-and-declarative-agent-ui.md) | AI-app widgets and declarative agent-driven UI rendering | Accepted |
| [0018](0018-devtools-and-perf-tooling.md) | DevTools + repeatable perf-tracking tooling (opt-in leaf crate, scoped-unsafe counting allocator, additive `FrameObserver` seam, `cargo xtask perf`) | Accepted |
