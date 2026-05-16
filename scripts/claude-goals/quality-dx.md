# Claude `/goal` Prompt: rstui Quality, Benchmarks, Profiling, And DX

You are Claude Code running one of five parallel streams for rstui. Your stream
is **Stream 5: Quality, benchmarks, profiling, kitchen sink, and developer
experience**.

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

- `crates/xtask/**`
- benchmarking/profiling infrastructure
- CI/check scripts and quality gates
- docs for conventions and developer workflows
- kitchen-sink/demo harness infrastructure and smoke-test workflows
- examples only when they are cross-cutting demos rather than a specific
  feature stream's implementation

Other streams are active in parallel:

- Stream 1 owns concrete widgets and third-party widget authoring.
- Stream 2 owns full-screen runtime/crossterm/app shells.
- Stream 3 owns rich document rendering.
- Stream 4 owns plugin host/runtime work around secure-exec.

Avoid editing those areas except for small compile/export integration. If you
need another stream's work, merge/rebase the latest `main` and build on it.
Do not duplicate their feature.

## Product Direction

rstui should be fast, maintainable, and easy for humans and agents to iterate
on. The project should have strong validation, strict linting, useful
benchmarks, memory and CPU profiling workflows, and eventually a kitchen sink
app that makes progress visible.

The kitchen sink is not required before enough UI exists, but when it becomes
useful it should make it easy to see what works and what does not. It should be
the place other streams can add demo panels without each one inventing a
separate app shell.

## Goal

Build the project-wide feedback loop:

- strict validation commands
- custom project checks
- vague-name enforcement
- benchmark/profiling scaffolding
- headless/e2e smoke workflows
- kitchen sink harness when enough feature surface exists
- documentation that makes agent iteration easy

Useful areas to choose from:

- `xtask bench` or a documented benchmark workflow
- criterion or iai-style decision ADR if dependencies are introduced
- snapshot comparison helpers
- kitchen sink skeleton that compiles and can accept feature panels from other
  streams
- CI docs/checklist
- profiling docs for macOS/Linux
- custom naming/check expansion
- stricter clippy/rustdoc policy if practical now
- e2e test guidance for full-screen apps and examples

Benchmark hot paths such as:

- buffer diffing
- layout solving
- text wrapping/rendering
- widget rendering
- terminal flush batching
- event/runtime loops
- larger app frames

Profiling should make memory and CPU behavior easy to inspect and keep stable.

## Engineering Rules

- Keep checks fast enough for local iteration, with heavier benchmarks/profiles
  clearly separated.
- Maintain and strengthen the vague-name ban: do not add buckets or identifiers
  like `helpers`, `utils`, `common`, `misc`, `shared`, `stuff`, `manager`, or
  similarly generic names unless an existing documented exception explicitly
  applies.
- Do not implement feature-owned widgets/runtime/plugin internals except as
  needed for cross-cutting harnesses.
- Keep the kitchen sink composable so feature streams can add demos without
  merge-heavy central rewrites.
- Make docs direct and command-oriented. Agents should be able to run the loop,
  see output, and know what failed.
- If adding dependencies, justify the cost and scope them to dev/bench tooling
  when possible.

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

Use Claude Code agent teams/background agents aggressively inside this stream's
ownership boundary. Parallelize independent work so the stream moves fast:
one agent can inspect benchmark/profiling/checking patterns, one can implement
a focused xtask/CI/docs slice, and another can verify validation commands,
docs, and smoke workflows. Keep ownership clear and integrate all agent work in
this stream worktree before committing. Do not spawn agents to edit other
streams' owned areas.

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your stream branch.
2. Pick a coherent quality/DX slice.
3. Implement it with tests/docs/checks.
4. Run validation.
5. Commit the slice.
6. Merge the slice back to `main` using the protocol below.
7. Continue with the next slice, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "Improve rstui quality workflow <feature>"
```

## Serialized Merge-Back Protocol

Use this exact intent after each validated commit. The lock is important
because five Claude sessions may be merging to `main` at the same time.

```sh
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
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

git -C "$main_repo" checkout main
git -C "$main_repo" pull --ff-only origin main
git -C "$main_repo" status --short
```

If the main checkout is dirty, stop and report. Do not stash, reset, or
overwrite it.

Merge and push:

```sh
git -C "$main_repo" merge --no-ff "$branch" -m "Merge quality DX goal slice"
(
  cd "$main_repo"
  cargo fmt --all --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-features
  cargo run -p xtask -- lint-names
)
git -C "$main_repo" push origin main
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

You are done only when you have shipped meaningful quality/DX progress,
validated it, merged it to `main`, pushed `main`, and left a short summary of
what landed plus any follow-up questions for the next iteration.
