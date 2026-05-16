Run the rstui Plugin Host and Secure Execution stream.

Repo: `/Users/andrewmcclenaghan/dev/andymac4182/rstui`
Full brief: `scripts/claude-goals/plugins.md`
Stream: `plugins`

First read the full brief from the repo and follow it as the source of truth.
Do not summarize it and stop. Execute it.

Goal: create your own worktree from latest `origin/main`, then make substantial,
validated progress on rstui's permissioned plugin system. Own plugin
host/runtime crates or modules, plugin ADRs/docs, examples/tests for manifests,
permissions, process isolation, IO boundaries, and focused runtime integration
only where plugin events/capabilities require it. Avoid widgets, rich
rendering, benchmark, CI, and kitchen-sink work except for tiny integration
needs.

Use `rivet-dev/secure-exec` as the security reference. Use OpenCode and
`earendil-works/pi` to understand plugin capabilities, extension points,
manifests, and user expectations. Prefer a small real host/runtime boundary
with deterministic tests over placeholders.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, commit, then merge yourself back to `main` using the serialized
`/tmp/rstui-main-merge.lock` protocol in the full brief, validate again on
`main`, and push `main`. Repeat until no useful next plugin slice remains or a
real blocker appears.

Maintain existing ADRs/conventions and the vague-name ban. Run the strongest
available gates: `cargo fmt --all --check`, `cargo clippy --all-targets
--all-features -- -D warnings`, `cargo test --all-features`, and
`cargo run -p xtask -- lint-names`.

Stop instead of forcing state if main is dirty, merge conflicts are ambiguous,
or validation cannot be made green.
