Run the rstui Rich Document Rendering stream.

Repo: `/Users/andrewmcclenaghan/dev/andymac4182/rstui`
Full brief: `scripts/claude-goals/rich-rendering.md`
Stream: `rich-rendering`

First read the full brief from the repo and follow it as the source of truth.
Do not summarize it and stop. Execute it.

Goal: use the current Claude `--worktree` if launched there, otherwise create
your own worktree from latest `origin/main`; then make substantial, validated
progress on rich document rendering. Own rich-rendering
widgets/modules under `crates/rstui-widgets/**`, or a clearly justified
optional rich-rendering crate if dependencies require it; own examples for
markdown, links, tables, Mermaid, and diffs; touch text/core primitives only
when truly needed. Avoid general widgets, runtime, plugins, benchmarks, CI, and
kitchen-sink work except for tiny integration needs.

Build progressive real slices for markdown rendering, clickable links, markdown
tables, Mermaid charts rendered to terminal-friendly ASCII/Unicode, and text
diffs inspired by `modem-dev/hunk`. Use `npx opensrc@latest path
github:modem-dev/hunk`, OpenTUI, and OpenCode as references when useful.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, commit, then merge yourself back to `main` using the serialized
`/tmp/rstui-main-merge.lock` protocol in the full brief, validate again on
`main`, and push `main`. Repeat until no useful next rich-rendering slice
remains or a real blocker appears.

Maintain existing ADRs/conventions and the vague-name ban. Run the strongest
available gates: `cargo fmt --all --check`, `cargo clippy --all-targets
--all-features -- -D warnings`, `cargo test --all-features`, and
`cargo run -p xtask -- lint-names`.

Stop instead of forcing state if main is dirty, merge conflicts are ambiguous,
or validation cannot be made green.
