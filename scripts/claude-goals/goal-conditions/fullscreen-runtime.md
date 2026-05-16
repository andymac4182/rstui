Run the rstui Full-Screen Runtime stream.

Main checkout: `/Users/andrewmcclenaghan/dev/andymac4182/rstui`
Full brief in this worktree: `scripts/claude-goals/fullscreen-runtime.md`
Stream: `fullscreen-runtime`

First confirm `git rev-parse --show-toplevel` is NOT the main checkout path
above. If it is, stop immediately. Then read the full brief from the current
worktree and follow it as the source of truth. Do not summarize it and stop.
Execute it.

Goal: use the current working directory as your stream worktree and make
substantial, validated progress on full-screen TUI runtime support. Own
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

All source edits must happen in the current worktree. Do not edit files under
the main checkout path directly; use it only for the merge-back commands.

Maintain existing ADRs/conventions and the vague-name ban. Validate with the
one command that runs every gate including the rustdoc `doc` gate:
`cargo run -p xtask -- ci` (fmt, lint-names, clippy, doc, test). Validate
the merged `main` checkout, not just your worktree, before pushing; never
push a red `main` — see `docs/merging.md`.

Stop instead of forcing state if main is dirty, merge conflicts are ambiguous,
or validation cannot be made green.
