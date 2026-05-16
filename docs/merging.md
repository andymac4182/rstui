# Merging a stream to `main`

rstui is built by parallel streams that each merge to one shared `main`
all session. The whole point is that every stream picks up the others'
work continuously — which only holds if **`main` is never red**. This is
the command-oriented checklist for getting a slice onto `main` without
breaking it for everyone else.

## The one rule

> **Validate the *merged* `main`, not just your worktree.**

A slice that is green on your branch can still turn `main` red, because
the *combination* of independently-green slices trips a gate (an
intra-doc link that only becomes ambiguous once another stream's module
lands, a `redundant_explicit_links` that only fires once another stream's
`use` brings the type into scope). Your worktree being green proves
nothing about `main` after the merge. Re-validate after the merge, in the
main checkout, every time.

## One-command preflight

Before the steps below, run:

```sh
cargo xtask merge-check
```

It enforces the worktree half of "the one rule" as a single GO/NO-GO that
cannot be half-done: you are on a stream branch, the tree is clean, the
branch is **rebased on the latest `origin/main`**, and **every** gate is
green (the full `cargo xtask ci`, `doc` included — the gate the old partial
list skipped). It deliberately does *not* take the lock, touch the main
checkout, or push: GO means "now do the serialized merge-back below"; the
merged-main re-validation in step 5 is still mandatory and still yours to
judge. NO-GO names the one thing to fix.

## Enforcement model

Two layers, deliberately:

- **Pre-push (prevention), by the brief.** `cargo xtask merge-check` plus the
  serialized protocol below. This is carried by the stream brief, which a
  session reads *once at start* — so a hardened protocol reaches a stream
  only on its next launch. New launches are correct by construction;
  already-running stragglers are not retroactively fixed, and that is
  accepted: the quality stream fix-forwards their breakage (the recurring
  rustdoc class has a known, mechanical fix below).
- **Post-push (backstop), by CI — authoritative.** `.github/workflows/ci.yml`
  runs on **every push to `main`** and every PR: the 5-gate `check` job
  *plus* the `msrv`, `unused-deps`, and `supply-chain` legs. A red `main`
  pushed by any stream is therefore caught by CI within minutes, regardless
  of whether that stream ran `merge-check`. CI re-running the exact gates on
  the pushed commit *is* the "the merged tree is green" check — a bespoke
  equivalent is not built because it would duplicate CI for no added signal
  (and the fix for a missed red `main` is the same fix-forward either way).

So: prevention is best-effort and improves every relaunch; detection is
guaranteed and immediate. The one thing never acceptable is *staying* red —
see below.

## Checklist

Set up:

```sh
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
worktree="$(git rev-parse --show-toplevel)"
branch="$(git branch --show-current)"
lock="/tmp/rstui-main-merge.lock"
```

1. **Acquire the serialized lock.** `mkdir "$lock"` is the atomic gate.
   If it exists, another stream is merging — wait. If it is *stale* (the
   recorded owner PID is dead), clear it and retry; do not block forever
   on a crashed stream:

   ```sh
   while ! mkdir "$lock" 2>/dev/null; do
     owner=$(cat "$lock/owner.pid" 2>/dev/null)
     if [ -n "$owner" ] && ! ps -p "$owner" >/dev/null 2>&1; then
       rm -f "$lock/owner.pid"; rmdir "$lock" 2>/dev/null || true; continue
     fi
     echo "waiting for another stream to finish merging (owner=$owner)..."
     sleep 20
   done
   echo "$$" > "$lock/owner.pid"
   ```

2. **Rebase on the *true* latest `origin/main`.** It moves under you
   while you work. Re-fetch immediately before the merge, not once at the
   start:

   ```sh
   cd "$worktree"
   git fetch origin main
   git rebase origin/main
   ```

3. **Re-validate on the rebased base.** Other streams' code is now
   *underneath* your commit — a clean rebase does not mean a clean build:

   ```sh
   cargo run -p xtask -- ci      # or: cargo xtask ci
   ```

4. **Sync the main checkout and confirm it is clean.** Never stash,
   reset, or overwrite a dirty main checkout — stop and report instead:

   ```sh
   git -C "$main_repo" checkout main
   git -C "$main_repo" pull --ff-only origin main
   git -C "$main_repo" status --short      # any output => STOP
   ```

5. **Merge, then validate the merged result in the main checkout.** This
   is the step the "one rule" is about. Only push if it is green:

   ```sh
   git -C "$main_repo" merge --no-ff "$branch" -m "Merge <stream> goal slice"
   ( cd "$main_repo" && cargo run -p xtask -- ci ) || {
       # not green: do NOT push. Restore main to exactly origin/main.
       git -C "$main_repo" reset --hard "$(git -C "$main_repo" rev-parse origin/main)"
       rmdir "$lock"; exit 1
   }
   git -C "$main_repo" push origin main
   ```

6. **Release the lock, then rebase your branch on the new `main`** so the
   next slice builds on everyone's just-landed work:

   ```sh
   rm -f "$lock/owner.pid"; rmdir "$lock"
   cd "$worktree" && git fetch origin main && git rebase origin/main
   ```

## When the merged `main` is red

A red `main` blocks every stream's loop, so it is a shared emergency, not
"someone else's bug". Two valid responses, in order of preference:

- **Fix forward** when the breakage is mechanical and side-effect-free —
  a doc-comment-only rustdoc fix, a formatting fix, a trivially-correct
  compile fix. Commit it as its own clearly-scoped slice ("Fix … gate:
  …"), re-validate, and include it in your merge-back so `main` goes
  green for everyone. Restoring a shared gate is quality work, not
  "forcing".
- **Stop and report** when the fix is ambiguous, requires real behavior
  changes, or touches another stream's logic in a non-trivial way. Do
  **not** push a red `main`; do **not** force state.

### The recurring rustdoc class

Almost every red `main` from a parallel merge has been one of these three
rustdoc gate failures. All are **doc-comment-only, zero behavior change**:

| Symptom | Fix |
|---|---|
| `redundant explicit link target` — `` [`X`](crate::X) `` | drop the target: `` [`X`] `` |
| `` `a::b` is both a function and a module `` (ambiguous link) | disambiguate: `` [`b`](a::b()) `` for the fn, `` [`b`](mod@a::b) `` for the module |
| `public documentation … links to private item` | unlink it: a plain `` `code` `` span, not `` [`code`] `` |

`cargo doc --no-deps --all-features --workspace` under
`RUSTDOCFLAGS=-D warnings` (gate 4 of `cargo xtask ci`) reproduces all of
them locally — which is exactly why step 5 exists.

## See also

- `cargo xtask merge-check` — the one-command preflight for everything
  above except the lock/push (which need judgment, never automation).
- [`docs/development.md`](development.md) — the fast inner loop
  (`cargo xtask ci`) this checklist guards.
- [`docs/benchmarking.md`](benchmarking.md) — the non-gating slow loop.
