# Claude `/goal` Prompt: rstui Full-Screen Runtime And App Shells

You are Claude Code running one of five parallel streams for rstui. Your stream
is **Stream 2: Full-screen runtime and app shells**.

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

Before editing, create your own worktree from current `origin/main`. Do not
work directly in the main checkout except during the serialized merge-back
protocol.

```sh
repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
stream="fullscreen-runtime"
stamp="$(date +%Y%m%d-%H%M%S)"
branch="goal/${stream}-${stamp}"
worktree_root="/Users/andrewmcclenaghan/dev/andymac4182/rstui-claude-worktrees"
worktree="${worktree_root}/${stream}-${stamp}"

git -C "$repo" fetch origin main
git -C "$repo" status --short
```

If the main checkout is dirty, stop and report the dirty files. Do not overwrite
or stash user work. If the worktree path already exists, choose a new suffixed
path. Do not delete existing worktrees.

```sh
mkdir -p "$worktree_root"
git -C "$repo" worktree add -b "$branch" "$worktree" origin/main
cd "$worktree"
```

## Your Ownership

Primary ownership:

- `crates/rstui-runtime/**`
- `crates/rstui-crossterm/**`
- runtime/crossterm examples, especially full-screen app examples
- focused `rstui-core` event/focus/terminal/event_source changes only when
  needed by the runtime boundary

Other streams are active in parallel:

- Stream 1 owns concrete widgets and third-party widget authoring in
  `rstui-widgets`.
- Stream 3 owns rich document rendering widgets and parsers.
- Stream 4 owns plugin host/runtime work around secure-exec.
- Stream 5 owns quality/DX infrastructure, benchmarks, profiling, kitchen sink,
  checks, CI, and developer workflows.

Avoid editing those areas except for small compile/export integration. If you
need another stream's work, merge/rebase the latest `main` and build on it.
Do not duplicate their feature.

## Product Direction

rstui should support full-screen TUI apps like OpenTUI does: alternate screen,
whole-terminal layout, robust lifecycle, responsive resize, keyboard/mouse
input, paste/focus events where supported, and terminal restoration even when
an app exits badly.

Learn from:

- `anomalyco/opentui`: how the most adopted/easy-to-work-with TUI framework
  shapes app lifecycle and developer iteration.
- `anomalyco/opencode`: real full-screen terminal app needs.
- `charmbracelet/bubbletea`: update/view/event loop ergonomics.
- `ratatui/ratatui`: backend abstraction, crossterm integration, and support
  for multiple backend strategies.

Use `npx opensrc@latest path github:<owner>/<repo>` when reference code would
help. Keep the implementation idiomatic to rstui's existing architecture.

## Goal

Make rstui excellent for full-screen terminal applications:

- alternate-screen lifecycle
- whole-terminal app shells
- resize handling
- focus/input routing
- mouse/paste/focus support where available
- panic-safe terminal restoration
- ergonomic app run loop
- deterministic headless end-to-end testing
- backend boundaries that can support multiple backends over time

The current crossterm backend is valuable, but keep the design open for other
backend/event-source implementations. Multiple backends let rstui support more
terminal environments, easier testing, alternative renderers, and lower-level
performance experiments without coupling every app to one terminal driver.

Useful areas to choose from:

- app shell helpers around `run`
- full-screen layout/frame lifecycle examples
- focus traversal and scoped input demos
- resize behavior tests
- mouse, paste, and terminal focus integration where crossterm supports it
- panic/cleanup robustness tests or docs
- richer event-loop ergonomics without hiding the Elm-style model
- backend capability traits or docs if current boundaries need clarification

## Engineering Rules

- Keep real terminal details behind backend/event-source crates.
- Preserve deterministic headless testing through `Harness`, `TestBackend`,
  and `TestEventSource`.
- Do not expand the widget catalog unless a small widget/demo is needed to
  prove runtime behavior.
- Maintain the vague-name ban: do not add buckets or identifiers like
  `helpers`, `utils`, `common`, `misc`, `shared`, `stuff`, `manager`, or
  similarly generic names unless an existing documented exception explicitly
  applies.
- Do not introduce broad async/runtime dependencies casually. If one is needed,
  document the reason and keep the boundary scoped.
- Keep APIs composable and documented enough that agents can build full-screen
  apps against them without guessing.
- Add end-to-end tests that drive real app behavior through the headless
  runtime whenever possible.

## Validation Before Each Commit

Run the strongest relevant validation you can before each commit:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo run -p xtask -- lint-names
```

Real TTY examples may be compile-checked rather than executed in CI/headless
contexts. If a command is unavailable because the repo has evolved, inspect the
current tooling and run the equivalent project gate.

## Work Loop

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your stream branch.
2. Pick a coherent runtime/app-shell slice.
3. Implement it with tests/examples/docs.
4. Run validation.
5. Commit the slice.
6. Merge the slice back to `main` using the protocol below.
7. Continue with the next slice, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "Improve rstui full-screen runtime <feature>"
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
git -C "$repo" merge --no-ff "$branch" -m "Merge full-screen runtime goal slice"
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

You are done only when you have shipped meaningful full-screen runtime progress,
validated it, merged it to `main`, pushed `main`, and left a short summary of
what landed plus any follow-up questions for the next iteration.
