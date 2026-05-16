# ADR 0009: Optional async-runtime policy (feature-gated tokio)

- **Status:** Superseded by [ADR 0011](0011-async-event-loop.md)
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** — (closes the "async / `EventStream`" item left
  deferred by [ADR 0008](0008-async-command-executor.md) §Follow-up and
  anticipated by [ADR 0001](0001-terminal-backend-strategy.md))

> **Superseded by [ADR 0011](0011-async-event-loop.md) (2026-05-17).** This
> ADR shipped `run_async` as "sync loop + tokio `spawn_blocking`" and
> *deferred* the async event loop. That deferral was reversed by an explicit
> maintainer decision to build the real `tokio::select!` event loop;
> ADR 0011 redefines the `async` feature accordingly and removes the
> strictly-inferior sync-loop variant (pre-1.0, feature-gated, no external
> users). The policy half of this ADR — tokio stays *optional, off by
> default, justified, documented; the default build is tokio-free* — is
> **carried forward unchanged** by ADR 0011. Retained as the historical
> record of why the interim step existed.

## Context

[ADR 0008](0008-async-command-executor.md) gave the runtime an off-loop
command executor (`run_threaded`, one `std::thread` per command) and a
bounded-pool variant (`run_pooled`), both dependency-free, and recorded
"`async`/`EventStream` executor: still deferred; the synchronous loop
never depends on tokio." [ADR 0001](0001-terminal-backend-strategy.md)
likewise anticipated "an async `EventStream` (crossterm `event-stream`,
tokio) behind a crate feature for when the runtime goes async."

The remaining open question is narrow and must now be **decided**, not
left vaguely deferred: does rstui take a tokio dependency, and if so on
what terms, given the brief's hard rule — *"Do not introduce broad
async/runtime dependencies casually. If one is needed, document the
reason and keep the boundary scoped."*

Two distinct things are conflated under "async" and must be separated:

1. An **async command executor** — running `Cmd::perform`/timer work on
   a tokio runtime's pool instead of raw `std::thread`s.
2. An **async event loop** — `poll_event` becoming `stream.next().await`,
   `run_core` becoming `async fn`, a futures-valued `Cmd`, and a
   correspondingly async (or re-justified) `Harness`.

These have very different cost and risk.

## Decision drivers

1. **Dependency discipline** — the default build, `Harness`, `run`,
   `run_threaded`, `run_pooled` must stay tokio-free.
2. **Scoped & justified** — any tokio use must be opt-in, minimal-
   feature, and documented (the brief's escape clause).
3. **One reducer** — no change to `settle`/`run_core`/the `Harness`
   contract; the async path must reuse the existing `CommandExecutor`
   seam.
4. **Decisiveness** — the open question is closed here, with a crisp,
   auditable boundary for what is delivered vs. deferred (not "TBD").
5. **Real value** — what ships must be genuinely useful, not a
   dependency-heavier alias of `run_pooled`.

## Options considered

### A. Feature-gated tokio command executor only (chosen)

A `default`-off `async` cargo feature pulls an **optional** `tokio`
(`default-features = false`, `features = ["rt"]` — `spawn_blocking` +
a current-thread runtime, no net/time/macros). It adds
`AsyncCommandExecutor` (the existing `pub(crate) CommandExecutor` seam,
via `tokio::task::spawn_blocking`) and a `run_async` entry that owns a
private current-thread runtime and drives the **unchanged** sync
`run_core`. No `settle`/`run_core`/`Harness`/`Cmd` change. The async
*event loop* and a futures-valued `Cmd` are explicitly **not** built.

- Honors every driver: opt-in, minimal feature, documented, one
  reducer, decisive. Real value: an app already on tokio runs its
  command work on that runtime's managed, instrumented, shared blocking
  pool (limits, `tracing`, metrics) instead of unmanaged threads — the
  integration point a tokio backend actually wants — while everyone
  else pays nothing.

### B. Full async event loop now (`EventStream`, async `run_core`, async `Harness`)

- Maximum async fidelity.
- But it is a **loop rewrite, not an executor addition**: `run_core`
  becomes `async`, the deterministic sync `Harness` either becomes
  async or stops being representative, and the `EventSource` contract
  changes. High risk, broad surface, and squarely the "casual broad
  async" the brief forbids. ADR 0001 already files this as a separate,
  unscheduled future. Rejected for now.

### C. No tokio at all; declare async permanently out of scope

- Simplest.
- But it leaves a real, repeatedly-raised integration gap (tokio
  backends) unaddressed and the question perpetually "deferred" rather
  than decided. Rejected: option A closes it cheaply and safely.

### D. Futures-valued `Cmd::perform_future` across all executors

- The most "complete" async API.
- But a feature-gated `Effect::Future` forces a gated `CommandExecutor`
  trait method on *every* impl, including the deterministic
  `InlineExecutor` (which would need a clock-free `block_on`) and the
  non-tokio thread/pool executors (where a tokio-reactor future panics).
  Large cross-cutting risk for a convenience over `spawn_blocking`.
  Deferred deliberately (see Consequences), not built.

## Decision

**Adopt Option A.** Add an off-by-default `async` feature gating an
**optional** minimal tokio (`rt` only). It provides
`AsyncCommandExecutor` over the existing `CommandExecutor` seam and a
`run_async` entry that owns a current-thread runtime and reuses the
unchanged sync `run_core`. The default build and every existing
entry point (`run`, `run_threaded`, `run_pooled`, `Harness`,
`rstui_crossterm::run_app`) remain exactly as before and tokio-free.

The workspace CI gate (`cargo xtask ci`) runs `--all-features`, so the
feature-gated code is fully fmt/clippy(`-D warnings`)/rustdoc(`-D
warnings`)/test-checked on every merge — the optional path cannot rot.

**Deliberately deferred, now with a crisp boundary (not "TBD"):**

- A **futures-valued `Cmd`** (`Cmd::perform_future`) — would require a
  feature-gated `Effect`/trait-method across all executors and a
  determinism story for the inline harness; Option D's risk. The
  `Send + 'static` seam keeps it non-breaking later.
- An **async event loop / `EventStream`** — Option B; a loop rewrite
  owned by ADR 0001's "not scheduled" future, not this ADR.

This ADR therefore *closes* the async question: the executor seam is
proven to extend to tokio and is shipped feature-gated; anything beyond
is a separate, named, deliberately-unscheduled architectural decision.

## Evidence

- **The brief**: "Do not introduce broad async/runtime dependencies
  casually. If one is needed, document the reason and keep the boundary
  scoped." Option A is the literal embodiment — optional, `rt`-only,
  documented here, default untouched.
- **ADR 0001** §Decision: "an async `EventStream` (crossterm
  `event-stream`, tokio) behind a crate feature … The synchronous path
  never pulls in tokio." Option A's feature gate is exactly that
  mechanism; the event loop remains 0001's separate future.
- **ADR 0008** §Consequences/Follow-up: the `CommandExecutor` seam was
  built so a new executor is "same seam, new impl; no API change."
  `AsyncCommandExecutor` is precisely that — no `settle`/`run_core`/
  `Harness` change, results on the same `mpsc` channel `run_core`
  already drains.
- **`crates/xtask/src/ci.rs`**: the clippy/doc/test gates run
  `--all-features`, so the gated tokio code is enforced green on every
  merge — the optional path is not an untested island.
- **tokio**: `spawn_blocking` accepts exactly the
  `FnOnce() + Send + 'static` the seam already carries and returns
  immediately; the result travels on the existing channel, so the loop
  is unchanged and tokio buys an app *already on tokio* a managed,
  instrumented, shared blocking pool over unmanaged `std::thread`s.

## Consequences

**Positive**

- The async question is **decided and closed**, not perpetually
  deferred; the boundary of what is/ isn't built is auditable.
- Apps already on tokio integrate command work into their runtime's
  managed pool with no reducer/loop change; everyone else pays nothing
  (default build never compiles tokio).
- The `CommandExecutor` seam is demonstrated to extend cleanly to a
  third executor backend, validating ADR 0008's design.
- CI's `--all-features` keeps the optional path permanently green.

**Negative / accepted**

- One optional dependency enters the workspace, isolated to
  `rstui-runtime`'s `async` feature and never in the default graph.
- `run_async` over sync `Cmd` closures is only marginally more capable
  than `run_pooled` for non-tokio apps; its value is specifically the
  tokio-integration case. Accepted — that *is* the gap being closed,
  and `run_pooled` remains the dependency-free choice otherwise.

**Neutral / deferred**

- Futures-valued `Cmd::perform_future`: deferred with rationale above;
  non-breaking later via the same seam.
- Async event loop / `EventStream`: remains ADR 0001's unscheduled
  future; explicitly *not* this ADR's scope.

## Follow-up

None required: this ADR closes the async-runtime question for the
synchronous framework. Re-open only via a new ADR if a concrete
consumer needs a futures-valued `Cmd` or the async event loop — both
have a recorded, non-breaking path through the existing seam.
