# Claude `/goal` Prompt: rstui Widgets And Component Authoring

You are Claude Code running one of five parallel streams for rstui. Your stream
is **Stream 1: Widgets and component authoring**.

You are allowed to work for a long time. Take bigger, coherent slices than a
normal short coding session. After every coherent validated slice, commit it on
your stream branch and merge it back to `main` using the merge protocol below
before continuing. The purpose is to let all streams benefit from each other's
work throughout the night instead of diverging for hours.

## Repository

The source repository is:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/rstui
```

Preferred launch mode is Claude's own `--worktree` flag. If this session was
already launched in a Claude-created worktree, use the current directory as
`$worktree` and do not create a second worktree. If you were launched from the
main checkout instead, create your own worktree from current `origin/main`.
Do not work directly in the main checkout except during the serialized
merge-back protocol.

```sh
repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
stream="widgets"
stamp="$(date +%Y%m%d-%H%M%S)"
branch="goal/${stream}-${stamp}"
worktree_root="/Users/andrewmcclenaghan/dev/andymac4182/rstui-claude-worktrees"
worktree="${worktree_root}/${stream}-${stamp}"

git -C "$repo" fetch origin main
git -C "$repo" status --short
```

If you are already in a Claude worktree, set `worktree="$(pwd)"`, set `branch`
from `git branch --show-current`, and continue. If you are in the main checkout
and the main checkout is dirty, stop and report the dirty files. Do not
overwrite or stash user work. If the worktree path already exists, choose a new
suffixed path. Do not delete existing worktrees.

```sh
mkdir -p "$worktree_root"
git -C "$repo" worktree add -b "$branch" "$worktree" origin/main
cd "$worktree"
```

## Your Ownership

Primary ownership:

- `crates/rstui-widgets/**`
- widget examples under `crates/rstui-widgets/examples/**`
- widget docs in README sections that describe concrete widgets
- focused `rstui-core` changes only when a widget needs a public primitive and
  that primitive clearly belongs in core

Other streams are active in parallel:

- Stream 2 owns full-screen runtime, crossterm lifecycle, app shells, event loop
  ergonomics, resize/input runtime behavior, and backend lifecycle.
- Stream 3 owns rich document rendering: markdown, clickable links, markdown
  tables, Mermaid-to-terminal output, and text diffs.
- Stream 4 owns plugin host/runtime work around secure-exec, plugin manifests,
  permissions, and process isolation.
- Stream 5 owns quality/DX infrastructure: benchmarks, profiling, xtask checks,
  lint policy, kitchen sink harness, CI, and developer workflows.

Avoid editing those areas except for small compile/export integration. If you
need another stream's work, merge/rebase the latest `main` and build on it.
Do not duplicate their feature.

## Product Direction

rstui should become the Rust TUI framework people reach for when they want to
build polished terminal apps quickly. Learn from:

- `anomalyco/opentui`: especially why it is easy to use and widely adopted.
- `anomalyco/opencode`: application-grade UI patterns and real terminal app
  needs.
- `charmbracelet/bubbletea`: update/view ergonomics.
- `ratatui/ratatui`: Rust terminal rendering boundaries and backend lessons.
- `longbridge/gpui-component`: breadth and polish of component APIs.

Use `npx opensrc@latest path github:<owner>/<repo>` when reference code would
help. Keep the work practical: inspect references, then implement idiomatic
rstui APIs.

## Goal

Build larger, coherent widget/component slices that make rstui feel productive
and easy to extend. Prioritize a full component set and third-party widget
ergonomics over tiny one-helper changes.

Concrete widgets belong in `rstui-widgets`; `rstui-core` keeps primitives and
the `Widget` trait. Keep widgets as pure projections of caller-owned state.
APIs should be composable, documented, and easy for humans and agents to copy
when building custom widgets.

Useful areas to choose from:

- textarea or multiline text entry
- select/dropdown
- tree
- command palette UI
- status bar
- notifications/toasts
- form composition primitives that do not own application state
- widget composition examples showing custom third-party widgets
- examples/tests that make widget output easy to inspect without a TTY

## Engineering Rules

- Follow existing ADRs and conventions in `docs/adr/**` and
  `docs/conventions/**`.
- Maintain the vague-name ban: do not add buckets or identifiers like
  `helpers`, `utils`, `common`, `misc`, `shared`, `stuff`, `manager`, or
  similarly generic names unless an existing documented exception explicitly
  applies.
- Keep widgets total: tiny areas, out-of-range state, empty data, and narrow
  widths should clip or no-op rather than panic.
- Keep state ownership with the caller. A widget renders state; reducers own
  mutation.
- Prefer snapshot/headless tests through the existing buffer/backend/harness
  model.
- Add focused docs when an API is intended for third-party widget authors.
- Do not introduce new dependencies casually. If one is necessary, justify it
  in code/docs and keep it scoped.

## Validation Before Each Commit

Run the strongest relevant validation you can before each commit:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo run -p xtask -- lint-names
```

If a command is unavailable because the repo has evolved, inspect the current
tooling and run the equivalent project gate. Do not claim success without
running validation or clearly recording why it could not run.

## Work Loop

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your stream branch.
2. Pick a coherent widget/component slice.
3. Implement it with tests/examples/docs.
4. Run validation.
5. Commit the slice.
6. Merge the slice back to `main` using the protocol below.
7. Continue with the next slice, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "Add rstui widget <feature>"
```

## Serialized Merge-Back Protocol

Use this exact intent after each validated commit. The lock is important
because five Claude sessions may be merging to `main` at the same time.

```sh
repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
lock="/tmp/rstui-main-merge.lock"

while ! mkdir "$lock" 2>/dev/null; do
  echo "Waiting for another rstui stream to finish merging to main..."
  sleep 20
done

cleanup_lock() {
  rmdir "$lock" 2>/dev/null || true
}
trap cleanup_lock EXIT
```

While holding the lock:

```sh
cd "$worktree"
git fetch origin main
git rebase origin/main
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo run -p xtask -- lint-names

git -C "$repo" checkout main
git -C "$repo" pull --ff-only origin main
git -C "$repo" status --short
```

If the main checkout is dirty, stop and report. Do not stash, reset, or
overwrite it.

Merge and push:

```sh
git -C "$repo" merge --no-ff "$branch" -m "Merge widgets goal slice"
(
  cd "$repo"
  cargo fmt --all --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-features
  cargo run -p xtask -- lint-names
)
git -C "$repo" push origin main
```

If merge conflicts occur, abort the merge in the main checkout, release the
lock, resolve the conflict in your stream worktree by rebasing on the latest
`origin/main`, rerun validation, recommit if needed, and then retry the
merge-back protocol. Do not push a broken `main`.

After a successful push, release the lock and continue from your stream
worktree:

```sh
trap - EXIT
rmdir "$lock"
cd "$worktree"
git fetch origin main
git rebase origin/main
```

## Definition Of Done

You are done only when you have shipped meaningful widget/component progress,
validated it, merged it to `main`, pushed `main`, and left a short summary of
what landed plus any follow-up questions for the next iteration.
