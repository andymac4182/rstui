RSTUI Stream 5: Quality, benchmarks, profiling, kitchen sink, and developer experience.

You are one of five parallel gnhf streams working on rstui. Stay in your lane so the branches merge cleanly.

Other streams:
- Stream 1 owns concrete widgets and third-party widget authoring.
- Stream 2 owns full-screen runtime/crossterm/app shells.
- Stream 3 owns rich document rendering.
- Stream 4 owns plugin host/runtime work around secure-exec.

Your primary ownership:
- `crates/xtask/**`
- benchmarking/profiling infrastructure
- CI/check scripts and quality gates
- docs for conventions and developer workflows
- kitchen-sink/demo harness infrastructure and smoke-test workflows
- examples only when they are cross-cutting demos rather than a specific feature stream's implementation

Goal:
Make rstui fast, maintainable, easy to inspect, and easy for agents/humans to iterate on. This stream owns the project-wide quality and feedback loop.

Important direction:
- Add benchmarking and profiling as first-class capabilities once there is enough surface for useful measurements.
- Benchmark hot paths such as buffer diffing, layout solving, text wrapping/rendering, widget rendering, terminal flush batching, event/runtime loops, and larger app frames.
- Provide memory and CPU profiling workflows that are easy to run locally.
- Enforce custom project checks, including the vague generic naming ban.
- Strengthen strict lint/rustdoc/check policy when practical.
- Build or prepare the kitchen sink app as the visible progress harness when enough functionality exists.
- Do not implement feature-owned widgets/runtime/plugin internals except as needed for cross-cutting harnesses.

Useful next areas:
- `xtask bench` or documented benchmark workflow
- criterion or iai-style decision ADR if dependencies are introduced
- snapshot comparison helpers
- kitchen sink skeleton that compiles and can accept feature panels from other streams
- CI docs/checklist
- profiling docs for macOS/Linux
- custom naming/check expansion

Validation:
Run the relevant cargo gates before success. Prefer `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, and any existing xtask checks.
