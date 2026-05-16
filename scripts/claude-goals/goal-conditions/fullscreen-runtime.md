Run the rstui Full-Screen Runtime stream.

Repo: `/Users/andrewmcclenaghan/dev/andymac4182/rstui`
Full brief: `scripts/claude-goals/fullscreen-runtime.md`
Stream: `fullscreen-runtime`

First read the full brief from the repo and follow it as the source of truth.
Do not summarize it and stop. Execute it.

Goal: use the current Claude `--worktree` if launched there, otherwise create
your own worktree from latest `origin/main`; then make substantial, validated
progress on full-screen TUI runtime support. Own
`crates/rstui-runtime/**`, `crates/rstui-crossterm/**`, runtime/crossterm
examples, and only focused `rstui-core` event/focus/terminal/event_source
changes needed by the runtime boundary. Avoid widget catalog, rich rendering,
plugin, benchmark, CI, and kitchen-sink work except for tiny integration needs.

Focus on OpenTUI-quality full-screen apps: alternate-screen lifecycle,
whole-terminal app shells, resize handling, focus/input routing, mouse/paste
support where available, panic-safe terminal restoration, ergonomic app run
loops, deterministic headless e2e testing, and backend boundaries that can
support multiple backends over time.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, commit, then merge yourself back to `main` using the serialized
`/tmp/rstui-main-merge.lock` protocol in the full brief, validate again on
`main`, and push `main`. Repeat until no useful next runtime slice remains or a
real blocker appears.

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
