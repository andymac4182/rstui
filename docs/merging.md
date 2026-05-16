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

- [`docs/development.md`](development.md) — the fast inner loop
  (`cargo xtask ci`) this checklist guards.
- [`docs/benchmarking.md`](benchmarking.md) — the non-gating slow loop.
