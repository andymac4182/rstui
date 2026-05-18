# ADR 0018: DevTools + repeatable perf-tracking tooling

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

The first perf review (`docs/perf-review.md`) was a one-shot, hand-run
audit: 15 parallel readers, a thesis, ~40 byte-identical fixes. It worked,
but it does not *repeat*. Since it landed the workspace has grown five
crates and a stream of features, and the only standing instrument is
`cargo xtask bench` — a dependency-free timing harness (ADR 0005) that
measures the *already-tuned core* (`buffer/diff`, `layout/split`) and a
handful of per-widget scenarios. It cannot answer the questions a living
project asks every week:

- Where did this frame's CPU actually go — `view`, `diff`, `flush`,
  `update`?
- How many heap allocations did that keystroke cost? Is something leaking
  across frames?
- What is the real FPS under load, and what is the input→frame latency
  when the mouse floods move events across the screen (the RT-01
  saturation class)?
- Did *this* change regress any of the above versus a known-good
  baseline?

And a second, distinct need: someone *building on* rstui has none of the
introspection a browser dev gets for free. They cannot see the frame
timeline, the dirty-region map, the allocation profile, or the event
latency of their own app without bolting on ad-hoc `eprintln!`s.

The forces this decision must fit (it relitigates none of them):

- **Pure projection, no retained widget tree (ADR 0012).** A widget is
  handed a `Buffer` and may not mutate; *all* state is caller-owned; the
  reducer is the only mutation point. Any perf state must therefore be
  caller-owned model state, exactly the `ScrollState`/`Input`/`Editor`
  seam — never interior widget state.
- **`rstui-core`/`-widgets`/`-runtime` are dependency-free and
  `unsafe_code = "forbid"` workspace-wide (ADR 0001/0003).** The single
  source of truth for lint policy is `[workspace.lints]`; every crate
  opts in with `[lints] workspace = true` (ADR 0003 §1). `forbid` cannot
  be lifted by any `#[allow]`.
- **Allocation counting needs a `#[global_allocator]`.** A `GlobalAlloc`
  impl is irreducibly `unsafe`. Batch J already hit this wall and
  *correctly* omitted the alloc counter: under workspace
  `unsafe_code = "forbid"` it cannot exist, and no `#[allow]` lifts a
  `forbid`. The first review's profiling guidance (`docs/benchmarking.md`)
  is consequently "use out-of-process heaptrack/Instruments" — accurate,
  but not the in-process, per-frame, zero-setup signal a repeatable
  review wants.
- **Bench is the slow loop, never a CI gate (ADR 0005).** Perf tooling
  must not enter `cargo xtask ci`'s five gates; it is opt-in and
  on-demand.

## Decision

Add **`rstui-devtools`**: an opt-in, dev/debug leaf crate plus one
additive runtime seam and one new `cargo xtask perf` workflow. Five
specific decisions:

### 1. A new leaf crate, never a dependency of the shipped libraries

`crates/rstui-devtools` depends on `rstui-core`/`-widgets`/`-runtime`,
**not the reverse**. `rstui-core`/`-widgets`/`-runtime`/`-crossterm`
gain *zero* dependencies and keep `unsafe_code = "forbid"` untouched.
A downstream app adds `rstui-devtools` as a normal (or dev-)dependency
and opts in explicitly. The forbid's guarantee for everyone *using*
rstui's core is therefore unchanged — the unsafe lives only in a crate
you choose to pull in to debug your own app.

### 2. Scoped-unsafe `CountingAllocator` — a deliberate, bounded ADR-0003 deviation

`rstui-devtools`'s `Cargo.toml` does **not** carry `[lints] workspace =
true`. It instead declares its own `[lints]` block that mirrors the
workspace clippy/rustdoc policy verbatim (same denials, `missing_docs`,
intra-doc-link denial) and differs in exactly one line:
`unsafe_code = "deny"` instead of `"forbid"`. `deny` keeps every
unaudited `unsafe` a hard error but — unlike `forbid` — permits a single
`#[allow(unsafe_code)]` on the one place it is unavoidable: the
`GlobalAlloc` impl. That impl is a thin pass-through to
`std::alloc::System` bracketed by atomic counters; it is a dozen lines,
has no raw-pointer arithmetic of its own, and is covered by an
exactness test (a known allocation pattern ⇒ exact counts). Every other
file in the crate is `unsafe`-free and is held to the *same* bar as the
rest of the workspace. This is the minimum viable deviation: not "a
crate where unsafe is fine" but "a crate where exactly one audited
allocator shim is permitted, everything else identical to workspace
policy."

### 3. An additive, pure `FrameObserver` runtime seam

`rstui-runtime` gains one optional, caller-supplied observer invoked
once per event-loop iteration with a by-value `FrameMetrics` (frame
number; `input`/`update`/`view`/`diff+flush` `Duration`s; the RT-01
`produced` flag; coalesced input-event count; input→frame latency
including the mouse-move-flood worst case). The observer **receives a
value and retains no widget state** — it is the ADR-0012 caller-owned
seam, not a retained tree. It is wired through the existing
`settle`/`handle_input`/`render` boundaries already timed with `Instant`.
The change is non-breaking: `run`/`run_threaded`/`run_pooled`/`Harness`
behave byte-identically when no observer is installed (the default), and
`Harness` additionally exposes the last `FrameMetrics` so perf is
deterministically testable headless. The phase timestamps are read with
`Instant::now()` only when an observer is present, so the zero-observer
path keeps its current cost.

### 4. `cargo xtask perf` — the repeatable review, not a gate

A new `cargo xtask perf` subcommand (dispatched exactly like
`bench`, **outside** the five CI gates per ADR 0005) runs the bench
suite, writes a markdown + JSON report, and diffs against a checked-in
`docs/perf-baseline.json`, flagging any scenario that regressed past a
threshold. "Do a perf review" becomes one command; `docs/perf-review.md`
stays the narrative, `perf-baseline.json` the machine record. The bench
registry is extended to close the BN02 gap the first review flagged:
per-widget render for the new hot widgets, an idle `view→diff→flush`
scenario via the public `Harness`, and an input/mouse-flood→frame-latency
scenario.

### 5. A DevTools overlay widget — Chrome-DevTools for a TUI

`rstui-devtools` ships a toggleable overlay `Widget` that is a *pure
projection* of a caller-owned `PerfSession` (ADR 0012): Performance
(per-phase frame timeline + FPS), Memory (alloc current/peak/per-frame
delta + a cross-frame leak indicator), Events (input→frame latency,
mouse-flood stalls), and Inspect (dirty-region heatmap / cell inspect /
projection stats). It is built only from existing `rstui-widgets`
primitives, so it adds no rendering machinery.

## Consequences

- One crate carries one audited `unsafe` allocator shim; the shipped
  libraries' `forbid` and dependency-free guarantees are untouched. The
  deviation is documented here and localized by construction (a leaf
  crate nothing in core depends on).
- Perf review is now `cargo xtask perf` + a baseline diff — repeatable,
  scriptable, regression-catching, still ADR-0005 non-gating.
- An app gets in-process CPU/alloc/FPS/latency introspection and a
  DevTools overlay by adding one dependency, one `#[global_allocator]`
  line, and one observer hookup — all opt-in, all caller-owned.
- The runtime gains exactly one additive seam; the zero-observer path is
  unchanged and unmeasured-cost.

## Alternatives considered

- **Out-of-process only (status quo: heaptrack/Instruments).** Kept and
  still documented — it is the right tool for a deep one-off dive. But it
  is not zero-setup, not per-frame, not in the app, and cannot drive a
  DevTools overlay or a CI-adjacent regression diff. Complementary, not
  sufficient.
- **A counting allocator inside `rstui-core` behind a feature.** Rejected:
  no `#[allow]` lifts the workspace `forbid`, and weakening it for the
  shipped core would erode the guarantee for *every* downstream user to
  serve a debug-only need. The leaf crate confines the cost to those who
  opt in.
- **A retained-tree devtools inspector (real Chrome-DevTools keeps a
  DOM).** Rejected: violates ADR 0012. The overlay is instead a pure
  projection of caller-owned `PerfSession` state, mirroring how every
  other rstui widget already works.
- **Make `cargo xtask perf` a sixth CI gate.** Rejected per ADR 0005:
  timing is environment-sensitive; gating on it makes CI flaky. The
  baseline diff is a *report*, run on demand, not a pass/fail gate.
