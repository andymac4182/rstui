# ADR 0006: Runtime tick and loop model

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

`rstui-runtime` is an Elm/Bubble-Tea-style loop. Until this slice it
was purely input-driven: `run` blocked on
`EventSource::poll_event(None)` and only ever woke when the terminal
delivered an event. A full-screen TUI of OpenTUI quality also needs to
do work *when nothing is typed* — advance a spinner, redraw a clock,
re-poll a background job, drive an animation frame. That periodic
dimension has to be added without breaking the constraints earlier
slices locked in:

- **`settle` is the single source of truth.** `run` (live) and
  `Harness` (headless) share one `settle` state machine; ADR 0001's
  end-to-end testing contract and the runtime's "the harness *is* the
  live loop with a `TestBackend` swapped in" guarantee both depend on
  that core not forking. Anything that changes how messages fold must
  change it in exactly one place or not at all.
- **`Cmd` runs inline.** `crates/rstui-runtime/src/cmd.rs` documents
  that commands are performed inline by `settle` (a slow
  `Cmd::perform` blocks the loop); offloading to a thread pool / async
  executor is a separately-scoped future slice. The `Send + 'static`
  bound exists only to keep that future seam non-breaking.
- **The `Harness` is clock-free and deterministic.** It has no TTY,
  threads, *or* wall clock. ADR 0001's testing layers L1–L3 assume a
  test advances the world explicitly (`handle`, `message`, `resize`),
  never by sleeping.
- **The seam was pre-reserved.** `crates/rstui-core/src/event_source.rs`
  already specifies `poll_event(Some(timeout))` as a bounded wait whose
  `Ok(None)` means "nothing this tick", and
  `crates/rstui-crossterm/src/event_source.rs` already implements that
  timed mode ("one poll and at most one read so an animation tick can
  never be starved") explicitly *"reserved for a future animation/tick
  slice"*.

The open question: what shape should the periodic mechanism take so it
is ergonomic, does not hide the Elm model, keeps `settle` single-
sourced, and keeps the headless harness clock-free?

## Decision drivers

1. **Single `settle` core** — the live and headless paths must not
   fork; ideally `settle`'s signature and body are untouched.
2. **Deterministic headless testing** — no wall clock may enter
   `Harness`; a test must advance time explicitly.
3. **Don't hide the Elm model** — periodic work must be expressed in
   the model/update vocabulary, not an opaque timer registry the app
   cannot reason about.
4. **Backward compatible** — every existing input-only app must behave
   byte-for-byte as before, with no signature break.
5. **Backend-agnostic** — the mechanism must ride the `EventSource`
   trait, not crossterm, so the multi-backend boundary ADR 0001 keeps
   open is not quietly closed.
6. **Minimal new surface** — prefer the smallest addition that covers
   the real cases (spinner, clock, animation frame, re-poll).

## Options considered

### A. `App::tick_rate(&self) -> Option<Duration>` + `App::on_tick(&self) -> Option<Message>` (an Elm *subscription*)

The app declares a cadence as a pure function of state and maps an
elapsed period to a message, exactly as `on_event` maps input. `run`
polls `poll_event(Some(until_next_tick))` while a rate is declared and
`poll_event(None)` otherwise; a bounded `Ok(None)` is a tick, an
unbounded `Ok(None)` is still end-of-input. `Harness::tick()` runs the
same `on_tick → update → settle → render` path with no clock.

- `settle` is **unchanged**. `on_tick` is `&self` (decides intent, the
  sole mutation stays `update`), structurally identical to the
  existing `on_event → update → settle` call sites. The wall clock
  (`Instant`) lives only in `run`, never in `settle` or `Harness`.
- Rides the already-reserved `poll_event(Some(_))` seam; no new runtime
  state, no timer registry, no thread.
- Backend-agnostic (it is the `EventSource` contract). Backward
  compatible: defaults are `None`, so an app overriding neither is the
  old input-only loop verbatim.
- Cost: one bounded and one unbounded poll path in `run`; a ticking app
  stops via `Cmd::quit`/error rather than bounded end-of-input
  (acceptable — see Evidence: Bubble Tea behaves identically).

### B. `Cmd::tick(Duration, FnOnce() -> M)` (a self-rescheduling effect, Bubble Tea's `tea.Tick`)

A scheduled one-shot effect; the app re-issues it from `update` to
repeat.

- Idiomatic to Bubble Tea and superficially "just another `Cmd`".
- But `Cmd` is **inline** here. A `Cmd::tick` is either (i) a blocking
  inline sleep that freezes the render loop — unacceptable — or (ii) a
  timer registry the runtime owns and `run` multiplexes via
  `poll_event(min(timers))`. Option (ii) forces timer state *through
  `settle`* (it must record pending timers as it drains effects),
  changing the one core `run` and `Harness` must agree on — precisely
  driver 1's failure. It also leaves the schedule invisible to tests
  unless the `Harness` grows a virtual clock (driver 2 regression).

### C. Both A and B

Maximum flexibility, but two timing systems to maintain and a real
footgun (an app mixing a declared rate and a scheduled tick). Violates
driver 6 with no case A does not already cover.

## Decision

**Adopt Option A.** The periodic mechanism is the Elm *subscription*
analog: `App::tick_rate(&self) -> Option<Duration>` declares the
cadence as a pure function of state, and `App::on_tick(&self) ->
Option<Self::Message>` is the temporal dual of `on_event`. The live
`run` loop derives its `poll_event` timeout from `tick_rate`; an
elapsed bounded wait maps through `on_tick → update → settle` exactly
as input maps through `on_event → update → settle`. `settle` is
untouched. `Harness::tick()` is the deterministic twin — the *same*
path with no wall clock, so a test advances time by *calling* `tick`.

A tick is deliberately **not** an `Event` variant: terminal input and
runtime timing are different concepts and, per the `event` module's
recorded discipline, never share a type.

Option B (`Cmd::tick`) and wall-clock-aligned `Every` are **deferred,
not rejected**. They become essentially free once the inline-command
limitation is lifted by the future async/threaded command slice; until
then Option A covers the spinner/clock/animation/re-poll cases with
zero new runtime state. This deferral is recorded here so the next
iteration does not re-derive it.

### Boundary consequence: multiple backends stay open

Because the tick rides `EventSource::poll_event(Some(_))` — a
`rstui-core` trait method, not a crossterm call — the periodic loop is
backend-agnostic. A future `rstui-termwiz` (or any `EventSource`)
inherits ticking with no runtime change, and the headless
`TestEventSource` needs no timer support because `Harness::tick()`
bypasses polling entirely. The runtime ↔ `Backend` ↔ `EventSource`
boundary ADR 0001 keeps open is therefore *not* narrowed by adding
ticks; this ADR is the record that it was checked.

## Evidence

Concrete facts from the reference projects and the rstui tree (cited so
the reasoning is auditable):

- **Bubble Tea** (`charmbracelet/bubbletea`, `pkg.go.dev`): `tea.Tick`
  is a *single-fire* command ("the timer begins precisely when invoked,
  and runs for its entire duration"); repeating requires returning
  another `Tick` from `Update`. `tea.Every` is single-fire but aligned
  to the system clock. A Bubble Tea program with a ticker runs until
  `tea.Quit` — it does not stop on input exhaustion. rstui's Option A
  ticking app stopping only via `Cmd::quit`/error is the *same*
  contract, so the bounded-`Ok(None)`-is-a-tick rule is not a rstui
  novelty.
- **Elm** (`guide.elm-lang.org/effects/time.html`): periodic time is a
  **subscription** (`Time.every`), a language construct *separate from
  `Cmd`*; the runtime injects subscription messages and tests inject
  them directly with no clock (`elm-program-test`). Option A is exactly
  the minimal subscription form — which is the *more* Elm-accurate
  model for periodic time than a self-rescheduling `Cmd`, so "don't
  hide the Elm model" (driver 3) favours A, not B.
- **Ratatui** (`ratatui.rs/concepts/event-handling`, async-template):
  the canonical synchronous pattern is `poll(tick_rate)` → on timeout
  emit a synthetic tick and redraw — precisely Option A over the
  `EventSource` seam. The async/`tokio::select!` variant is the
  equivalent of the deferred Option B and only appears once a project
  takes the async dependency, mirroring this ADR's deferral.
- **rstui tree**: `crates/rstui-core/src/event_source.rs` already
  documents the two meanings of `Ok(None)` and reserves
  `poll_event(Some(timeout))`; `crates/rstui-crossterm/src/event_source.rs`
  already implements the starvation-free timed mode "reserved for a
  future animation/tick slice"; `crates/rstui-runtime/src/cmd.rs`
  records that `Cmd` is inline and async is a separate slice. Option A
  consumes exactly the seam that was pre-built for it and respects the
  constraint that makes B premature.

## Consequences

**Positive**

- `settle` is byte-identical; the "harness is the live loop" guarantee
  and ADR 0001's deterministic testing story hold unchanged.
- The headless `Harness` stays clock-free; `Harness::tick()` makes
  animation as unit-testable as any keypress (the `spinner` and
  `app_shell` examples assert whole animations with no TTY or sleep).
- Backward compatible: `tick_rate`/`on_tick` default to `None`, so
  every prior app is unaffected and the feature is strictly opt-in.
- Cadence is state-driven, so an app starts/stops/retunes animation by
  what `tick_rate` returns (no separate "stop the timer" plumbing), and
  goes back to a zero-cost input-only block when idle.
- Multiple backends remain implementable behind the unchanged
  `EventSource` trait (boundary explicitly re-verified above).

**Negative / accepted**

- A wall clock (`std::time::Instant`) enters `run` — but only `run`,
  the component that already does real IO; it never reaches `settle` or
  `Harness`. Accepted and documented in `run.rs`.
- A ticking app cannot stop via bounded end-of-input; it must
  `Cmd::quit` or error. Accepted: identical to Bubble Tea, and the
  unbounded EOF path is unchanged for non-ticking apps.
- Missed ticks coalesce (a frame slower than the rate does not schedule
  a catch-up storm); there is no wall-clock-aligned `Every`. Accepted
  for animation/poll use; `Every` rides the deferred slice.

**Neutral / deferred**

- `Cmd::tick(duration, fn)` and clock-aligned `Every` are deferred to
  the async/threaded command slice that makes them free; this ADR is
  the record of *why*, so it is not re-litigated.
- Per-frame animation driven by `Frame::count()` (a deterministic frame
  clock) already exists for phase; `tick_rate` is what *drives* frames
  when there is no input. The two compose; no change needed.

## Follow-up

This ADR is the reference contract for the next runtime slices:

1. The async/threaded command slice: when `Cmd` is no longer inline,
   add `Cmd::tick`/`Every` as the scheduled-effect form, superseding
   the "deferred" note here (a new ADR links back).
2. Signal-aware restore (SIGTERM/SIGWINCH) is a distinct concern from
   ticking and is not covered here.
