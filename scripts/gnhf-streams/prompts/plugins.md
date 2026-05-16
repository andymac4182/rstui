RSTUI Stream 4: Plugin host and secure execution.

You are one of five parallel gnhf streams working on rstui. Stay in your lane so the branches merge cleanly.

Other streams:
- Stream 1 owns concrete widgets and third-party widget authoring.
- Stream 2 owns full-screen runtime/crossterm/app shells.
- Stream 3 owns rich document rendering.
- Stream 5 owns quality/DX infrastructure, benchmarks, profiling, kitchen sink, checks, and CI/dev workflows.

Your primary ownership:
- plugin host/runtime crates or modules
- plugin ADRs/docs
- examples/tests that demonstrate plugin manifests, permissions, process isolation, and IO boundaries
- focused runtime integration only when needed for plugin events/capabilities

Goal:
Design and implement rstui's permissioned plugin system using rivet-dev/secure-exec as the security reference, while learning from OpenCode and pi plugin models.

References:
- `npx opensrc@latest path github:rivet-dev/secure-exec`
- `npx opensrc@latest path github:anomalyco/opencode`
- `npx opensrc@latest path github:earendil-works/pi`

Important direction:
- Security and testability are first-class.
- Prefer a small real host/runtime boundary with deterministic tests over placeholder crates.
- Model permissions, capabilities, process isolation, IO contracts, and manifest/configuration explicitly.
- Do not couple plugin internals to specific widgets unless a minimal demo requires it.
- Avoid owning the kitchen sink/demo infrastructure; Stream 5 owns that, though plugin demos can plug into it later.
- Maintain the vague-name ban.

Useful next areas:
- plugin architecture ADR if not already present
- manifest/capability model
- permissioned command execution proof of concept
- safe input/output protocol for plugins
- deterministic fake plugin runner for tests
- failure and timeout behavior

Validation:
Run the relevant cargo gates before success. Prefer `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, and any existing xtask checks.
