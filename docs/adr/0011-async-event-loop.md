# ADR 0011: Async event loop (`tokio::select!`)

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** [ADR 0009](0009-optional-async-runtime-policy.md)

## Context

[ADR 0009](0009-optional-async-runtime-policy.md) added a feature-gated
`run_async` that was, deliberately, only a *command executor* change:
the event loop stayed synchronous (`run_core` blocking on
`EventSource::poll_event(timeout)`), tokio merely ran command closures
on `spawn_blocking`. ADR 0009 explicitly *deferred* the async event
loop and recorded it as a separate, unscheduled decision (echoing
[ADR 0001](0001-terminal-backend-strategy.md)).

A maintainer has now explicitly directed that rstui have the **best
architecture for a fast TUI**, and specifically that the async event
loop be built. This ADR makes that decision and supersedes ADR 0009.

The cost of the sync loop for a "fast TUI" is concrete and measurable:
`run_threaded`/`run_pooled`/`run_async` (0009) all wake on
`COMMAND_POLL_INTERVAL` (~16 ms) to drain off-loop command results,
because a blocking `poll_event` cannot also await a channel. That is
up-to-16 ms added latency on every async command/result and a steady
~60 Hz wakeup even when the UI is idle. A true async loop that
`select!`s input, results, and ticks reacts to whichever is ready
*immediately* and is genuinely idle (zero wakeups) when nothing is
happening.

The hard constraint is unchanged from ADR 0008/0009: **one reducer**
(`settle`), one `App` contract, and a deterministic headless `Harness`
— none of which may regress.

## Decision drivers

1. **Latency / idle cost** — eliminate the poll-interval; idle = no
   wakeups (the "fast TUI" requirement).
2. **One reducer, zero drift** — `settle`/`step`/`App`/`Harness`
   unchanged; the async loop is *another driver* over the same reducer,
   exactly as `run_threaded` is to `run`.
3. **Determinism preserved** — the sync `Harness` still deterministically
   tests app logic (it drives the same `settle`); the async loop's own
   plumbing must be testable with **no real clock**.
4. **Dependency discipline (carried from ADR 0009)** — tokio stays
   optional, off by default, minimal-feature, documented; the default
   build and every sync entry point stay tokio-free.
5. **No two confusing async entry points** — pre-1.0, feature-gated, no
   external users: redefine, do not accumulate.

## Decision

**Build the async event loop and make it the meaning of the `async`
feature**, superseding ADR 0009's sync-loop `run_async`:

- Add `AsyncEventSource` — the async dual of `EventSource`:
  `async fn next_event(&mut self) -> Result<Option<Event>, Self::Error>`
  (native async-fn-in-trait; Rust 1.85). One unambiguous meaning per
  outcome — `Ok(None)` is *only* permanent end-of-input — because ticks
  are a separate `select!` arm, not a poll-timeout overload. It must be
  cancel-safe (a `select!` drops the losing future).
- Redefine `run_async` as an `async fn` that `tokio::select!`s, `biased`
  (input → results → ticks, so input never starves), over: the
  `AsyncEventSource`, a `tokio::sync::mpsc` of command results fed by a
  new `TokioCommandExecutor`, and a `tokio::time::interval` armed when
  `tick_rate()` is `Some` (missed ticks coalesce via
  `MissedTickBehavior::Skip`). Every arm calls the **same sync**
  `step`/`settle`/`render` — no `await` in the reducer path.
- Remove the ADR 0009 sync-loop `run_async` and its `spawn_blocking`-
  over-`std::mpsc` executor (pre-1.0, feature-gated, unused externally):
  one async entry point, one clear meaning.
- The **policy half of ADR 0009 is carried forward unchanged**: tokio is
  `optional`, off by default, `default-features = false`, minimal
  features (`rt`, `sync`, `time`, `macros`, `test-util`), justified and
  documented; `run`/`run_threaded`/`run_pooled`/`Harness`/the default
  build are tokio-free and untouched.
- `rstui_crossterm::run_app` stays on the sync `run_threaded` (no
  dependency); the async path is opted into explicitly (a future
  `run_app_async` over a crossterm `EventStream` source — separate
  slice).

Still **deliberately not** done (recorded so it is decided, not "TBD"):
a futures-valued `Cmd::perform_future` (the cross-executor
determinism problem ADR 0009 §Option D records; the `Send + 'static`
seam keeps it non-breaking later). The async loop does not require it.

## Evidence

- **`crates/rstui-runtime/src/run.rs`** — `settle`/`step` already take a
  `&mut dyn CommandExecutor`; the async loop reuses them verbatim, and
  `TokioCommandExecutor` is just another `CommandExecutor` impl (no
  `settle` change), confirming ADR 0008's "same seam, new impl".
- **crossterm `EventStream`** (feature `event-stream`) is a plain
  `futures::Stream<Item = io::Result<Event>>`, executor-agnostic — it
  drops cleanly into `AsyncEventSource` via the existing, tested
  `from_crossterm` map, with no change to the deterministic event
  vocabulary.
- **tokio**: `select!` with `biased`, `Interval::tick`, and
  `mpsc::Receiver::recv` are all documented cancel-safe, which is what
  makes the loss-of-a-`select!`-branch sound; `start_paused` virtual
  time makes the loop deterministically testable with no wall clock —
  the async analogue of the sync `Harness` guarantee.
- **`crates/xtask/src/ci.rs`** — the gate runs `--all-features`, so the
  gated async loop is fmt/clippy(`-D`)/rustdoc(`-D`)/test enforced on
  every merge; the default (no `async`) build never compiles tokio.
- **ADR 0009** itself recorded the sync-loop step as interim and
  pre-1.0; superseding it now (no external users) is the documented
  ADR convention, not churn.

## Consequences

**Positive**

- No poll-interval latency; the loop is idle (zero wakeups) when
  nothing happens — the fast-TUI win, on the framework's primary hot
  path (input).
- The reducer is untouched: `run`/`run_threaded`/`run_pooled`/the async
  loop all fold messages through the *same* `settle`, so the headless
  `Harness` stays the exact deterministic test of app logic. Only
  IO/effect multiplexing is async.
- One async entry point with one meaning; the tokio dependency is now
  justified by a genuine architectural benefit, not a managed pool.
- Determinism intact end-to-end: app logic via `Harness`; async-loop
  plumbing via `#[tokio::test(start_paused = true)]` (no real clock).

**Negative / accepted**

- `run_async` is now `async fn` and must be awaited inside a tokio
  runtime (the caller's, or a future `run_app_async`). Accepted: that
  is inherent to an async loop and is exactly the opt-in boundary.
- ADR 0009's sync-loop `run_async` is removed (a breaking change *only*
  under the off-by-default `async` feature, pre-1.0, no external users).
  Accepted and recorded here.
- `AsyncEventSource::next_event` must be cancel-safe. Documented on the
  trait; channel/stream-backed sources (the only realistic ones)
  satisfy it.

**Neutral / deferred**

- `Cmd::perform_future`: still deferred (ADR 0009 §Option D rationale);
  non-breaking later via the same seam.
- A crossterm `EventStream` `AsyncEventSource` + `run_app_async` shell
  entry: the natural next slice (separate, additive, default untouched).

## Follow-up

1. `rstui-crossterm`: a feature-gated `CrosstermAsyncEventSource`
   (crossterm `event-stream`) implementing `AsyncEventSource`, plus a
   `run_app_async` that owns a tokio runtime and composes guard +
   async source + `run_async`.
2. Re-open `Cmd::perform_future` only via a new ADR if a concrete
   consumer needs it (recorded non-breaking path through the seam).
