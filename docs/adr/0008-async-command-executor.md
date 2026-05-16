# ADR 0008: Off-loop command executor (threads, no async dependency)

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** — (resolves the `Cmd`-timer / off-loop deferral recorded
  in [ADR 0006](0006-runtime-tick-and-loop-model.md))

## Context

[ADR 0006](0006-runtime-tick-and-loop-model.md) added the steady
*subscription* tick (`tick_rate`/`on_tick`) and explicitly **deferred**
the Bubble-Tea-style scheduled `Cmd::tick`/`Cmd::every` and off-loop
command execution, because at the time `Cmd` ran *inline* inside the
shared `settle` reducer: a slow `Cmd::perform` blocked rendering, and a
`Cmd::tick(d, …)` could only be added by either blocking the loop or
threading a timer registry through `settle` — which would fork the one
state machine `run` and the headless `Harness` must share.

`cmd.rs` has documented from the start that command closures are
`Send + 'static` *specifically* to make an off-loop executor a
non-breaking future addition, and ADR 0006's follow-up named "the
async/threaded command slice" as the unblocking step. This ADR is that
slice.

The constraints that still bind:

- **One reducer.** `run`, `run_threaded`, and `Harness` must fold
  messages through the *same* `settle`; only *effect dispatch* may
  differ. ADR 0001's end-to-end testing contract depends on this.
- **Deterministic, clock-free `Harness`.** Tests must not sleep or
  depend on a wall clock or thread scheduling.
- **No casual broad dependency.** The brief forbids pulling in a
  broad async runtime (tokio) casually; if one is needed it must be
  scoped and justified.
- **Backward compatibility.** Every existing app (and the default
  `run`) must behave byte-for-byte as before unless it opts in.

## Decision drivers

1. **Single `settle` core** — unchanged reducer; the executor is the
   only thing that varies.
2. **Determinism** — `Harness` (and default `run` over a scripted
   source) stay clock-free and reproducible.
3. **Responsiveness** — a slow `Cmd::perform` or a real `Cmd::tick`
   delay must not freeze input or rendering.
4. **Dependency discipline** — prefer the standard library; no tokio
   for the synchronous loop.
5. **Backward compatibility** — opt-in, additive, no signature break
   for `run`/`App`/`Harness`.

## Options considered

### A. A `CommandExecutor` seam: inline (default/Harness) vs. `std::thread` (opt-in `run_threaded`)

`settle` takes a `&mut dyn CommandExecutor`. `InlineExecutor` runs the
closure now and yields `Some(message)` → folded this turn (the exact
pre-async behavior; used by `Harness` and the default `run`).
`ThreadCommandExecutor` spawns a `std::thread` per command, returns
`None`, and the message arrives on an `mpsc` channel the
`run_threaded` loop drains and re-feeds through the same `settle`.
`Cmd::tick`/`Cmd::every` are the timer effects; the inline executor
collapses their delay to zero (deterministic), the threaded one
actually sleeps (and `every` snaps to the wall clock).

- One reducer, untouched. Determinism preserved (inline = today;
  delays collapse to zero). No external dependency (std threads +
  `mpsc`). Opt-in via a distinct `run_threaded` entry; `run`/`App`/
  `Harness` signatures unchanged.
- Cost: a thread per in-flight command (not a pool); the threaded loop
  wakes at a bounded interval to drain results so it stops on
  `Cmd::quit`/error rather than bounded end-of-input (the same rule a
  ticking app already follows, ADR 0006).

### B. tokio / async `EventStream` executor

- Idiomatic async, a real reactor, cancellation.
- But pulls a broad runtime into the synchronous framework, against the
  brief's explicit guidance and ADR 0001's "the synchronous loop never
  depends on tokio." Disproportionate for "don't block the loop."

### C. Make `Cmd::perform` itself off-loop (no opt-in)

- Simplest surface (no second entry point).
- But changes message ordering for *every* existing app and would force
  the `Harness` to model asynchrony to stay representative — breaking
  determinism (driver 2) and backward compatibility (driver 5).

### D. A timer/registry threaded through `settle`

- Keeps one entry point.
- But mutates the shared reducer's shape (driver 1) — exactly what
  ADR 0006 refused. Rejected there, still rejected.

## Decision

**Adopt Option A.** Introduce a crate-internal, effectively-sealed
`CommandExecutor` seam with two implementations:

- `InlineExecutor` — runs every `perform`/timer closure inline, now,
  collapsing any delay to zero. Used by the headless `Harness` and the
  default `run`, so both remain byte-for-byte the pre-async loop and
  clock-free deterministic.
- `ThreadCommandExecutor` — one `std::thread` per command (Bubble
  Tea's per-goroutine model), delivering the message over an `mpsc`
  channel the new opt-in `run_threaded` loop drains and folds through
  the *same* `settle`. Dependency-free.

Add `Cmd::tick(Duration, FnOnce)->M` (relative one-shot, `tea.Tick`)
and `Cmd::every(Duration, FnOnce)->M` (next wall-clock multiple,
`tea.Every`). Repetition is the app re-issuing the command from
`update`, exactly as Bubble Tea does. The public surface added is
exactly `Cmd::tick`, `Cmd::every`, and `run_threaded`; the executor
trait stays `pub(crate)` (no speculative public surface — applications
choose behavior by which `run`/`run_threaded` they call, never by
implementing an executor).

A bounded thread pool, and an `async` executor, are explicitly
**deferred, not stubbed**: the `Send + 'static` bound keeps both
non-breaking, and the per-thread model is correct and adequate now.

## Evidence

- **`crates/rstui-runtime/src/cmd.rs`** has documented since the first
  slice that the `Send + 'static` bound exists so "the real runtime can
  run commands off the render loop … requiring it now keeps that future
  seam from being a breaking change." This ADR consumes exactly that
  pre-built seam.
- **ADR 0006** §Options/B and §Follow-up named the inline-`Cmd`
  limitation as the precise reason `Cmd::tick` was premature and
  identified "the async/threaded command slice that makes it free" as
  the unblocking step. The deferral's stated precondition is now met.
- **Bubble Tea** (`charmbracelet/bubbletea`): commands run on
  goroutines and deliver a `Msg` back to the update loop;
  `tea.Tick`/`tea.Every` are single-fire commands the app reschedules.
  `ThreadCommandExecutor` is the direct Rust analog (thread ≈
  goroutine, `mpsc` ≈ msg channel), so existing TUI intuition transfers
  and the inline/threaded split mirrors Bubble Tea's own test harness
  resolving commands synchronously.
- **ADR 0001** records "the synchronous loop never depends on tokio";
  the std-thread executor honors that while still removing the
  block-the-loop limitation, so no decision driver there is violated.

## Consequences

**Positive**

- A slow `Cmd::perform` or a real `Cmd::tick`/`Cmd::every` no longer
  freezes rendering or input under `run_threaded` — the responsiveness
  ADR 0006 deferred.
- `settle` is unchanged; `run`, `run_threaded`, and `Harness` share one
  reducer, so the harness stays an exact stand-in (behavior matches up
  to *when*, not *whether*, an off-loop message lands).
- Determinism intact: `Harness` and default `run` are inline and
  clock-free; the new `tick`/`every` fire immediately there, so
  effect-driven tests need no clock (the elm-program-test / Bubble Tea
  pattern).
- No external dependency; the brief's async-dependency guidance is
  honored. Fully backward compatible and opt-in.

**Negative / accepted**

- One thread per in-flight command (no pool). Adequate for typical TUI
  command volume; a pool is a future refinement the `Send + 'static`
  bound keeps non-breaking.
- `run_threaded` never blocks unbounded (it wakes every
  `COMMAND_POLL_INTERVAL` to drain results), so it stops on
  `Cmd::quit`/error, not bounded end-of-input — the same contract a
  ticking app already has (ADR 0006), and a real terminal never EOFs
  anyway.
- A command thread may outlive a quit; its channel send then fails
  silently (the receiver is gone), which is the correct, benign
  outcome.

**Neutral / deferred**

- Bounded thread pool: deferred, non-breaking later.
- `async`/`EventStream` executor: still deferred; the synchronous loop
  never depends on it.

## Follow-up

- A bounded pool executor, if command volume ever warrants it (same
  seam, new `CommandExecutor` impl; no API change).
- Wiring `rstui_crossterm::run_app` onto `run_threaded` so full-screen
  apps get off-loop commands by default is a small, separate backend
  slice (kept out of here to preserve `run_app`'s current semantics
  until decided deliberately).
