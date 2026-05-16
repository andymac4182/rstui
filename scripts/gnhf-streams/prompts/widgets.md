RSTUI Stream 1: Widgets and component authoring.

You are one of five parallel gnhf streams working on rstui. Stay in your lane so the branches merge cleanly.

Other streams:
- Stream 2 owns full-screen runtime, crossterm lifecycle, app shells, event loop ergonomics, and resize/input runtime behavior.
- Stream 3 owns rich document rendering: markdown, links, tables, Mermaid-to-terminal output, and text diffs.
- Stream 4 owns plugin host/runtime work around secure-exec, plugin manifests, permissions, and process isolation.
- Stream 5 owns quality/DX infrastructure: benchmarks, profiling, xtask checks, lint policy, kitchen sink harness, and CI/dev workflows.

Your primary ownership:
- `crates/rstui-widgets/**`
- widget examples under `crates/rstui-widgets/examples/**`
- widget docs in README sections that describe concrete widgets
- focused core changes only when a widget needs a public primitive, and only when that primitive clearly belongs in `rstui-core`

Goal:
Build larger, coherent widget/component slices that make rstui feel productive and easy to extend. Prioritize a full component set and third-party widget ergonomics over tiny one-helper changes.

Important direction:
- Concrete widgets belong in `rstui-widgets`; `rstui-core` keeps primitives and the `Widget` trait.
- Keep widgets pure projections of caller-owned state.
- APIs should be composable, documented, and easy for humans and agents to copy when building custom widgets.
- Maintain the vague-name ban: no buckets like helpers/utils/common/misc/shared unless an exception is documented.
- Use headless snapshot-style examples/tests so changes are easy to inspect without a real terminal.

Useful next areas:
- textarea
- select/dropdown
- tree
- command palette UI
- status bar
- notifications/toasts
- form composition helpers that do not own application state
- widget composition examples showing custom third-party widgets

Validation:
Run the relevant cargo gates before success. Prefer `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, and any existing xtask checks.
