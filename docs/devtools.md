# DevTools — live, in-app performance introspection

`rstui-devtools` is the **live** counterpart to `cargo xtask bench` /
`cargo xtask perf` (the offline loop, see
[`docs/benchmarking.md`](benchmarking.md)). It is what you reach for when
you are *building an app on rstui* and want to see, inside the running
program, exactly where a frame's time and memory go — the Chrome
DevTools experience for a TUI.

It is an **opt-in leaf crate** ([ADR 0018](adr/0018-devtools-and-perf-tooling.md)):
it depends on `rstui-core`/`-widgets`/`-runtime` and nothing depends on
it. Adding it changes nothing about the shipped libraries — they keep
`unsafe_code = "forbid"`; the one audited allocator shim lives here, in
the crate you opt into.

```sh
cargo run -p rstui-devtools --example devtools_demo
```

renders all four tabs over a deterministic `TestBackend` (it doubles as a
snapshot smoke test). The kitchen sink wires the *live* overlay behind
`F12`.

## The three pieces

### 1. Allocation tracking — one line

A global allocator shim over the system allocator that counts every
allocation/free and live/peak bytes with relaxed atomics. Install it once
in your **binary** (not a library):

```rust,ignore
#[global_allocator]
static GLOBAL: rstui_devtools::alloc::CountingAllocator =
    rstui_devtools::alloc::CountingAllocator::system();
```

Then anywhere, `rstui_devtools::alloc::snapshot()` reads the counters and
`snap.delta(&earlier)` gives the bytes/allocs a span of work cost.
Snapshotting only reads atomics — it never allocates — so it is safe to
call inside a frame observer. Without the allocator installed every count
is a static zero (the overlay's Memory tab just shows zeros — still
well-formed), so the allocator is genuinely optional.

### 2. `PerfMeter` — caller-owned per-frame history

`PerfMeter` is model state you own (the ADR-0012 §P1 `FpsMeter` /
`ScrollState` seam — *not* a retained widget tree), typically behind an
`Rc` so the frame observer can write it while your `view` reads it:

```rust,ignore
use std::rc::Rc;
use rstui_devtools::{PerfMeter, DevToolsAdapter};

let perf = Rc::new(PerfMeter::with_capacity(240)); // ring of the last 240 frames
let app  = MyApp::new(Rc::clone(&perf));           // app holds it, reads it in `view`
let mut observer = DevToolsAdapter::new(&perf);    // the runtime writes it

// Any runtime entrypoint has a `*_with_observer` twin (ADR 0018 §3); the
// zero-observer path is byte-identical and pays no timing cost.
rstui_runtime::run_threaded_with_observer(app, backend, &mut events, &mut observer)?;
// also: run_with_observer (inline) / run_pooled_with_observer (bounded pool)
```

`DevToolsAdapter` implements `rstui_runtime::FrameObserver`; on every
loop iteration the runtime hands it a `FrameMetrics` (per-phase
`logic`/`view`/`flush` durations, the RT-01 `produced` flag, coalesced
event count, input→frame latency) and it pairs that with the
`CountingAllocator` heap delta into the meter.

Headless (tests): `rstui_runtime::Harness::last_frame()` exposes the same
`FrameMetrics`, so you can assert performance properties deterministically
without a terminal.

Read the history back through the scoped accessor (the borrow must not
outlive an `on_frame`):

```rust,ignore
perf.with_session(|s| {
    println!("{:.0} fps, frame p99 {:?}", s.fps(), s.aggregate(|f| f.total).p99);
});
```

### 3. `DevTools` — the overlay

A pure projection of a borrowed `PerfMeter`, built only from existing
`rstui-widgets` primitives. The selected tab and whether it shows at all
are ordinary caller-owned state your `update` toggles from a hotkey —
never widget-driven (ADR 0012 §P1):

```rust,ignore
// in `view`, gated behind your own `show_devtools` bool:
if self.show_devtools {
    DevTools::new(&self.perf)
        .tab(self.devtools_tab)            // caller-owned 0..4
        .block(Block::bordered().title(" DevTools "))
        .render(area, frame.buffer_mut()); // drawn over your UI
}
```

## Reading the tabs

Four panes, mirroring a browser's (`rstui_devtools::overlay::TABS`):

| Tab | What it answers | Chrome analogy |
|---|---|---|
| **Performance** | Where does the frame go? Per-phase p50/p99 (`logic`≈Scripting, `view`≈Rendering, `flush`≈Painting), FPS, a frame-time history strip. | Performance |
| **Memory** | Is the app leaking per frame? Live/peak heap, window alloc bytes, allocs-vs-frees, and a **LEAK SUSPECT** verdict when net-live keeps growing with allocs > frees. | Memory |
| **Events** | Does input stall the UI? input→frame p50/p99, the worst stall, max events coalesced into one frame, and the **RT-01** line: how many no-op floods were skipped (a high skip count under pointer motion with low latency is the saturation guard *working* — this is the "freeze while moving the mouse" detector). | (input latency) |
| **Inspect** | A one-glance session summary: frames recorded vs all-time, repainted/skipped, fps, every phase's p50/p99. | (summary) |

## Relationship to `cargo xtask perf`

They are complementary, not redundant:

- `cargo xtask perf` (offline) — deterministic, repeatable, diffs a
  checked-in baseline, regression-flags in CI-adjacent runs. Use it to
  *prove a change did/didn't regress a hot path* and to do a periodic
  review ([`docs/perf-review-2.md`](perf-review-2.md) is the worked
  example).
- `rstui-devtools` (live) — your *actual* app, *real* workload, *real*
  input, real allocations. Use it to *find* where a janky interaction or
  a per-frame leak is, in situ, before you can even write a bench for it.

Find it live with DevTools, lock it down with a bench + baseline.

## Why no VHS / snapshot gate

The bench harness is deliberately non-gating because timing is
environment-sensitive ([ADR 0005](adr/0005-benchmarking-and-profiling-strategy.md));
the same is true *squared* for a live overlay whose Memory/Events numbers
depend on the host allocator and scheduler. So DevTools is **not** in the
VHS pipeline: snapshotting wall-clock/allocation numbers would be
anti-deterministic. The `devtools_demo` example is the determinism story
instead — it feeds the meter *fixed* `FrameMetrics` over a `TestBackend`,
so its rendered tabs are reproducible and it doubles as a smoke test,
exactly the rstui example convention (cf. `fps_counter_demo`). The live
overlay is for a human watching a real session.

---

See [ADR 0018](adr/0018-devtools-and-perf-tooling.md) for the
architecture (why a separate crate, the scoped-`unsafe` allocator, the
additive `FrameObserver` runtime seam, the pure-projection overlay).
