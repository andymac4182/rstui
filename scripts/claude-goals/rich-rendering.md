# Claude `/goal` Prompt: rstui Rich Document Rendering

You are Claude Code running one of five parallel streams for rstui. Your stream
is **Stream 3: Rich document rendering**.

You are allowed to work for a long time. Take bigger, coherent slices than a
normal short coding session. After every coherent validated slice, commit it on
your stream branch and merge it back to `main` using the merge protocol below
before continuing. The purpose is to let all streams benefit from each other's
work throughout the night instead of diverging for hours.

## Repository

The main checkout is:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/rstui
```

The launcher creates an explicit git worktree under:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/rstui-stream-worktrees
```

Treat the current working directory as the only editable source workspace.
Set these variables before running merge-back commands:

```sh
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
worktree="$(git rev-parse --show-toplevel)"
branch="$(git branch --show-current)"
```

If `worktree` is the same path as `main_repo`, stop immediately. You were
launched in the wrong directory. Do not edit the main checkout. The main
checkout is only the serialized merge-back target.

## Your Ownership

Primary ownership:

- rich-rendering widgets/modules under `crates/rstui-widgets/**`, or a clearly
  justified rich-rendering crate if dependencies require it
- examples that demonstrate markdown, links, tables, Mermaid, or diffs
- focused text/core primitives only when needed by rich rendering

Other streams are active in parallel:

- Stream 1 owns the general concrete widget catalog and third-party widget
  authoring.
- Stream 2 owns full-screen runtime/crossterm/app-shell behavior.
- Stream 4 owns plugin host/runtime work around secure-exec.
- Stream 5 owns quality/DX infrastructure, benchmarks, profiling, kitchen sink,
  checks, CI, and developer workflows.

Avoid editing those areas except for small compile/export integration. If you
need another stream's work, merge/rebase the latest `main` and build on it.
Do not duplicate their feature.

## Product Direction

rstui needs first-class support for document-heavy terminal UIs. Future
complete functionality should include:

- markdown rendering
- clickable links and link activation
- markdown tables
- Mermaid charts rendered into terminal-friendly ASCII/Unicode output
- text diffs inspired by `modem-dev/hunk`

References:

- `npx opensrc@latest path github:modem-dev/hunk`
- `npx opensrc@latest path github:anomalyco/opentui`
- `npx opensrc@latest path github:anomalyco/opencode`

Use references to understand product feel and API shape, then implement narrow,
real vertical slices in rstui. Do not add a giant placeholder renderer that
pretends to be complete.

## Goal

Build the track for rich document rendering while fitting it into the normal
rstui widget/component model.

Useful areas to choose from:

- an initial `Markdown` or `MarkdownView` widget with headings, emphasis, code,
  block quotes, and list support
- markdown table rendering as a focused slice
- link span model and activation event shape
- text diff widget/reference implementation inspired by hunk
- Mermaid AST/input placeholder plus a narrow renderable subset
- examples and snapshot tests for document-heavy terminal output

If dependencies are required, keep them isolated. A separate rich-rendering
crate can be justified if it prevents optional parser dependencies from
becoming part of the base widget crate, but do not split crates without a clear
module boundary and real benefit.

## Engineering Rules

- Keep interactive pieces integrated with focus/input/event handling.
- Make output testable through headless snapshots.
- Keep APIs composable and documented enough that agents can build document UIs
  from them.
- Maintain the vague-name ban: do not add buckets or identifiers like
  `helpers`, `utils`, `common`, `misc`, `shared`, `stuff`, `manager`, or
  similarly generic names unless an existing documented exception explicitly
  applies.
- Avoid broad runtime/backend/plugin/benchmark work except for small
  integration needs.
- Prefer progressive fidelity: ship useful markdown/diff/table slices with
  tests rather than broad unsupported promises.
- Keep rendering deterministic and terminal-width aware.

## Validation Before Each Commit

Run the strongest relevant validation you can before each commit:

```sh
cargo run -p xtask -- ci   # fmt, lint-names, clippy, doc, test — the doc
                            # (rustdoc) gate is the one the partial list
                            # omitted and that turned `main` red.
```

If a command is unavailable because the repo has evolved, inspect the current
tooling and run the equivalent project gate. Do not claim success without
running validation or clearly recording why it could not run.

## Work Loop

Use Claude Code agent teams/background agents aggressively inside this stream's
ownership boundary. Parallelize independent work so the stream moves fast:
one agent can inspect hunk/OpenTUI/OpenCode rendering patterns, one can
implement a focused markdown/table/link/diff slice, and another can verify
snapshots/examples/docs. Keep ownership clear and integrate all agent work in
this stream worktree before committing. Do not spawn agents to edit other
streams' owned areas.

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your stream branch.
2. Pick a coherent rich-rendering slice.
3. Implement it with tests/examples/docs.
4. Run validation.
5. Commit the slice.
6. Merge the slice back to `main` using the protocol below.
7. Continue with the next slice, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "Add rstui rich rendering <feature>"
```

## Serialized Merge-Back Protocol

Use this exact protocol after each validated commit. The lock serializes the
five Claude sessions that may merge to `main` at once.

**The one rule: validate the _merged_ `main`, not just your worktree.** A
slice green on your branch can still turn `main` red, because the
*combination* of independently-green slices trips a gate (most often the
rustdoc `doc` gate). Your worktree being green proves nothing about `main`
after the merge. The canonical expanded checklist, with a table of the
recurring failures and their exact fixes, is `docs/merging.md` — read it.

Acquire the lock. `mkdir` is the atomic gate; a *stale* lock (its owner PID
is dead) is cleared so a crashed stream cannot block everyone forever:

```sh
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
lock="/tmp/rstui-main-merge.lock"

while ! mkdir "$lock" 2>/dev/null; do
  owner=$(cat "$lock/owner.pid" 2>/dev/null)
  if [ -n "$owner" ] && ! ps -p "$owner" >/dev/null 2>&1; then
    rm -f "$lock/owner.pid"; rmdir "$lock" 2>/dev/null || true; continue
  fi
  echo "Waiting for another rstui stream to finish merging to main..."
  sleep 20
done
echo "$$" > "$lock/owner.pid"

cleanup_lock() { rm -f "$lock/owner.pid"; rmdir "$lock" 2>/dev/null || true; }
trap cleanup_lock EXIT
```

While holding the lock, rebase on the true-latest `origin/main` (it moves
under you) and re-validate on the rebased base with the **one command that
runs every gate**. `cargo xtask ci` is fmt + lint-names + clippy + **doc** +
test, exactly CI's set; the older partial `fmt`/`clippy`/`test`/`lint-names`
list omits the rustdoc `doc` gate and is what let `main` go red:

```sh
cd "$worktree"
git fetch origin main
git rebase origin/main
cargo run -p xtask -- ci

git -C "$main_repo" checkout main
git -C "$main_repo" pull --ff-only origin main
git -C "$main_repo" status --short
```

If the main checkout is dirty, stop and report. Do not stash, reset, or
overwrite it.

Merge, then **validate the merged main checkout — this is the step the one
rule is about**:

```sh
git -C "$main_repo" merge --no-ff "$branch" -m "Merge rich rendering goal slice"
( cd "$main_repo" && cargo run -p xtask -- ci )
```

If that `cargo xtask ci` is **not green, DO NOT push** — the merged `main`
is red and blocks every stream. Either:

- **fix forward** when the breakage is mechanical and side-effect-free (a
  doc-comment-only rustdoc fix is the recurring case — `docs/merging.md` has
  the fix table): commit it as its own `Fix … gate: …` slice, re-run
  `cargo run -p xtask -- ci` in the main checkout until green, then push; or
- **restore and stop** when the fix is ambiguous or needs a real behavior
  change: `git -C "$main_repo" reset --hard origin/main`, release the lock,
  and report. Never push a red `main`; never force.

Push only once the merged main checkout is green:

```sh
git -C "$main_repo" push origin main
```

If merge conflicts occur, abort the merge in the main checkout, release the
lock, resolve in your worktree by rebasing on the latest `origin/main`,
rerun `cargo run -p xtask -- ci`, recommit if needed, then retry. Do not
push a broken `main`.

After a successful push, release the lock and continue from your stream
worktree, rebasing so the next slice builds on every stream's just-landed
work:

```sh
trap - EXIT
rm -f "$lock/owner.pid"; rmdir "$lock"
cd "$worktree"
git fetch origin main
git rebase origin/main
```

## Definition Of Done

You are done only when you have shipped meaningful rich-rendering progress,
validated it, merged it to `main`, pushed `main`, and left a short summary of
what landed plus any follow-up questions for the next iteration.
