Run the rstui Widgets stream.

Repo: `/Users/andrewmcclenaghan/dev/andymac4182/rstui`
Full brief: `scripts/claude-goals/widgets.md`
Stream: `widgets`

First read the full brief from the repo and follow it as the source of truth.
Do not summarize it and stop. Execute it.

Goal: use the current Claude `--worktree` if launched there, otherwise create
your own worktree from latest `origin/main`; then make substantial, validated
progress on rstui widget/component authoring. Own
`crates/rstui-widgets/**`, widget examples, widget docs, and only focused
`rstui-core` primitives that widgets truly need. Avoid runtime, plugin, rich
rendering, benchmark, CI, and kitchen-sink work except for tiny integration
needs.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, commit, then merge yourself back to `main` using the serialized
`/tmp/rstui-main-merge.lock` protocol in the full brief, validate again on
`main`, and push `main`. Repeat until no useful next widget slice remains or a
real blocker appears.

Keep widgets pure projections of caller-owned state, composable, documented,
agent-friendly, total under tiny/narrow/empty inputs, and consistent with the
existing ADRs/conventions. Maintain the vague-name ban. Run the strongest
available gates: `cargo fmt --all --check`, `cargo clippy --all-targets
--all-features -- -D warnings`, `cargo test --all-features`, and
`cargo run -p xtask -- lint-names`.

Stop instead of forcing state if main is dirty, merge conflicts are ambiguous,
or validation cannot be made green.
