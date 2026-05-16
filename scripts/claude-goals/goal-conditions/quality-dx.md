Run the rstui Quality, Benchmarks, Profiling, and DX stream.

Repo: `/Users/andrewmcclenaghan/dev/andymac4182/rstui`
Full brief: `scripts/claude-goals/quality-dx.md`
Stream: `quality-dx`

First read the full brief from the repo and follow it as the source of truth.
Do not summarize it and stop. Execute it.

Goal: use the current Claude `--worktree` if launched there, otherwise create
your own worktree from latest `origin/main`; then make substantial, validated
progress on project-wide quality and feedback loops. Own
`crates/xtask/**`, benchmark/profiling infrastructure, CI/check scripts,
quality gates, conventions/dev workflow docs, kitchen-sink/demo harness
infrastructure, and cross-cutting smoke workflows. Avoid feature-owned widget,
runtime, rich-rendering, and plugin internals except for tiny harness
integration needs.

Focus on strict validation, custom checks, vague-name enforcement, fast local
iteration, benchmarking hot paths, memory/CPU profiling workflows, headless/e2e
smoke tests, and a composable kitchen sink harness when enough feature surface
exists.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, commit, then merge yourself back to `main` using the serialized
`/tmp/rstui-main-merge.lock` protocol in the full brief, validate again on
`main`, and push `main`. Repeat until no useful next quality/DX slice remains
or a real blocker appears.

Use Claude Code agent teams/background agents to parallelize reference review,
implementation, and verification inside this stream's ownership boundary. Move
fast, integrate the agent work yourself, and do not let subagents edit other
streams' owned areas.

Maintain existing ADRs/conventions and the vague-name ban. Run the strongest
available gates: `cargo fmt --all --check`, `cargo clippy --all-targets
--all-features -- -D warnings`, `cargo test --all-features`, and
`cargo run -p xtask -- lint-names`.

Stop instead of forcing state if main is dirty, merge conflicts are ambiguous,
or validation cannot be made green.
