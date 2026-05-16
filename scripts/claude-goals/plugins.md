# Claude `/goal` Prompt: rstui Plugin Host And Secure Execution

You are Claude Code running one of five parallel streams for rstui. Your stream
is **Stream 4: Plugin host and secure execution**.

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

- plugin host/runtime crates or modules
- plugin ADRs/docs
- examples/tests that demonstrate plugin manifests, permissions, process
  isolation, and IO boundaries
- focused runtime integration only when needed for plugin events/capabilities

Other streams are active in parallel:

- Stream 1 owns concrete widgets and third-party widget authoring.
- Stream 2 owns full-screen runtime/crossterm/app shells.
- Stream 3 owns rich document rendering.
- Stream 5 owns quality/DX infrastructure, benchmarks, profiling, kitchen sink,
  checks, CI, and developer workflows.

Avoid editing those areas except for small compile/export integration. If you
need another stream's work, merge/rebase the latest `main` and build on it.
Do not duplicate their feature.

## Product Direction

rstui should eventually support powerful plugins like OpenCode and pi while
keeping execution permissioned and testable.

References:

- `npx opensrc@latest path github:rivet-dev/secure-exec`
- `npx opensrc@latest path github:anomalyco/opencode`
- `npx opensrc@latest path github:earendil-works/pi`
- `npx opensrc@latest path github:anomalyco/opentui` when UI integration
  lessons are useful

Use `rivet-dev/secure-exec` as the security reference. Use OpenCode and pi to
understand plugin capability models, extension points, manifest/configuration
shape, and user expectations.

## Goal

Design and implement rstui's permissioned plugin system. Security and
testability are first-class. Prefer a small real host/runtime boundary with
deterministic tests over placeholder crates.

Useful areas to choose from:

- plugin architecture ADR if not already present
- manifest/capability model
- permissioned command execution proof of concept
- safe input/output protocol for plugins
- deterministic fake plugin runner for tests
- timeout, failure, cancellation, and stderr/stdout behavior
- host integration points for app/runtime events without coupling to a specific
  widget

## Engineering Rules

- Model permissions, capabilities, process isolation, IO contracts, and
  manifest/configuration explicitly.
- Keep plugin internals decoupled from specific widgets unless a minimal demo
  requires it.
- Avoid owning the kitchen sink/demo infrastructure. Stream 5 owns the harness,
  though plugin demos can plug into it later.
- Maintain the vague-name ban: do not add buckets or identifiers like
  `helpers`, `utils`, `common`, `misc`, `shared`, `stuff`, `manager`, or
  similarly generic names unless an existing documented exception explicitly
  applies.
- Do not add unsafe execution paths. If a command can run code or touch the
  filesystem/network, make the permission boundary visible and testable.
- Keep APIs composable and documented enough that agents can build plugins and
  plugin-aware TUIs without guessing.
- Use deterministic tests with fake process/plugin runners wherever possible.

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
one agent can inspect secure-exec/OpenCode/pi plugin models, one can implement
a focused manifest/permission/runtime slice, and another can verify security
tests/docs/failure modes. Keep ownership clear and integrate all agent work in
this stream worktree before committing. Do not spawn agents to edit other
streams' owned areas.

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your stream branch.
2. Pick a coherent plugin/security slice.
3. Implement it with tests/examples/docs.
4. Run validation.
5. Commit the slice.
6. Merge the slice back to `main` using the protocol below.
7. Continue with the next slice, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "Add rstui plugin <feature>"
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
git -C "$main_repo" merge --no-ff "$branch" -m "Merge plugin host goal slice"
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

You are done only when you have shipped meaningful plugin/security progress,
validated it, merged it to `main`, pushed `main`, and left a short summary of
what landed plus any follow-up questions for the next iteration.
