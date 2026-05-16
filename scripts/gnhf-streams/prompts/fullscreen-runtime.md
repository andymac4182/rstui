RSTUI Stream 2: Full-screen runtime and app shells.

You are one of five parallel gnhf streams working on rstui. Stay in your lane so the branches merge cleanly.

Other streams:
- Stream 1 owns concrete widgets and third-party widget authoring in `rstui-widgets`.
- Stream 3 owns rich document rendering widgets and parsers.
- Stream 4 owns plugin host/runtime work around secure-exec.
- Stream 5 owns quality/DX infrastructure, benchmarks, profiling, kitchen sink, checks, and CI/dev workflows.

Your primary ownership:
- `crates/rstui-runtime/**`
- `crates/rstui-crossterm/**`
- runtime/crossterm examples, especially full-screen app examples
- focused `rstui-core` event/focus/terminal/event_source changes only when needed by the runtime boundary

Goal:
Make rstui excellent for full-screen TUI applications like OpenTUI: alternate-screen lifecycle, whole-terminal app shells, resize handling, focus/input routing, mouse/paste support where available, panic-safe terminal restoration, and an ergonomic app run loop.

Important direction:
- Keep real terminal details behind backend/event-source crates.
- Preserve deterministic headless testing through `Harness`, `TestBackend`, and `TestEventSource`.
- Do not expand the widget catalog unless a small widget/demo is needed to prove runtime behavior.
- Avoid touching plugin, rich-rendering, benchmark, or kitchen-sink infrastructure except for integration points.
- Maintain the vague-name ban.

Useful next areas:
- app shell helpers around `run`
- full-screen layout/frame lifecycle examples
- focus traversal and scoped input demos
- resize behavior tests
- mouse/paste/focus integration where crossterm supports it
- panic/cleanup robustness tests or docs
- richer event-loop ergonomics without hiding the Elm-style model

Validation:
Run the relevant cargo gates before success. Prefer `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, and any existing xtask checks. Real TTY examples may be compile-checked rather than executed in CI/headless contexts.
