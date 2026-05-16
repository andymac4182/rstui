# Benchmarking and profiling rstui

Measure the `rstui-core` hot paths and find where a frame spends its time.

This is the **slow loop**. The fast loop is `cargo xtask ci` (see
[`docs/development.md`](development.md)) and it runs on every commit. Benchmarks
and profiling are kept out of that gate on purpose so it stays fast —
[ADR 0005](adr/0005-benchmarking-and-profiling-strategy.md). Reach for this loop
only when you touch a hot path.

## Run it

Always go through the `xtask` wrapper: it forces a `--release` build (a debug
build reports meaningless numbers) and pins the same toolchain `xtask` ran with.
No extra `--` is needed — the cargo alias already supplies one.

```sh
cargo xtask bench
```

Runs every scenario.

```sh
cargo xtask bench buffer/diff
```

Keeps only scenarios whose name **contains** the argument as a substring, so
`buffer/diff` selects the whole `buffer/diff/*` family and `buffer/diff/full`
selects one. No match exits non-zero and tells you to use `--list`.

```sh
cargo xtask bench --list
```

Prints the scenario names, one per line, and exits.

```sh
cargo xtask bench --help
```

Prints usage (flags, env vars) and exits.

## Read the output

The header echoes the scenario count, iterations, warmup, and build profile,
then a fixed-width table:

```
scenario                                  min       median         mean
```

- **min** — the fastest single measured iteration: the sample least polluted by
  scheduler noise. Each iteration is timed individually, so one hiccup inflates
  one sample, not the run. `min` is the most stable signal across machines and
  runs — **compare `min` run to run to spot a regression**.
- **median** — the lower-middle sample.
- **mean** — the arithmetic mean over every measured iteration.

Units auto-scale per value (`ns` / `µs` / `ms` / `s`, two decimals).

This is a **deterministic timing aid, not a statistical benchmark**: no outlier
rejection, no confidence intervals, no regression database. It is enough to
eyeball an order-of-magnitude regression in a hot path; it is not a substitute
for a statistical harness when one is genuinely needed (ADR 0005 records that
escape hatch).

Two env vars tune the loop; a non-positive or unparsable value falls back to
the default so a typo never silently runs zero iterations:

| Variable | Default | Effect |
|---|---|---|
| `RSTUI_BENCH_ITERS` | `1000` | Measured iterations per scenario |
| `RSTUI_BENCH_WARMUP` | `100` | Untimed warmup iterations before measuring |

```sh
RSTUI_BENCH_ITERS=20000 cargo xtask bench buffer/diff/full
```

## Baseline (indicative, not a gate)

A reference point so a future run has something concrete to diff against —
**not** a pass/fail threshold (ADR 0005 keeps benchmarking non-gating, and a
machine-relative number can never be a low-churn gate per ADR 0003). Trust the
*shape* (relative cost, order of magnitude), not the absolute µs: your
hardware will differ. `min` is the stable signal.

Captured on an **Apple M1 Pro** (macOS, arm64), `--release`, default 1000
iters / 100 warmup, against the fixed 160×48 frame:

| Scenario | `min` |
|---|---|
| `buffer/diff/identical` | ~39 µs |
| `buffer/diff/sparse` | ~41 µs |
| `buffer/diff/full` | ~39 µs |
| `buffer/diff/resized` | ~44 µs |
| `buffer/fill` | ~2.6 µs |
| `buffer/set_str` | ~3.4 µs |
| `buffer/clear_region` | ~3.9 µs |
| `layout/split/nested` | ~0.42 µs |

The headline: a full-frame `diff` (~40 µs, scanning all 7 680 cells) dominates
every per-frame cost here — it is the hot path to watch. To refresh this table
after a deliberate change to a hot path, re-run on a quiet machine:

```sh
cargo xtask bench
```

and update the `min` column (round to two significant figures — finer
precision is noise).

### Refresh cadence

Refresh **opportunistically**, in the same slice that deliberately changes a
hot path the table covers (a `Buffer`/`Layout` change, a new scenario) — that
slice already has the numbers in hand and the diff explains the move.
Otherwise leave it static: it is *indicative, not a gate*, so a stale entry
costs nothing and a number that drifts on its own (machine, toolchain) is
noise, not signal. There is deliberately no scheduled refresh and no CI job
that regenerates it — that would be churn against a value ADR 0003 minimises.
If the table looks obviously wrong on a glance, re-run `cargo xtask bench`
and update it as its own one-line slice.

## The scenarios

All scenarios run against a fixed **160×48** frame (a large-but-ordinary
terminal, 7 680 cells) so numbers are comparable run to run. The eight names
are the `--list` and substring-filter vocabulary:

| Scenario | What it measures |
|---|---|
| `buffer/diff/identical` | Idle steady-state redraw: two identical frames, zero changes, every cell still compared. |
| `buffer/diff/sparse` | Small update — one changed row (a status line or cursor blink). |
| `buffer/diff/full` | Full repaint or scroll: every cell differs. |
| `buffer/diff/resized` | Resize invalidation: areas differ, so the whole surface is re-emitted. |
| `buffer/fill` | Allocate and fill a fresh frame-sized grid. |
| `buffer/set_str` | Per-frame text stamping widgets pay: a styled line into every row. |
| `buffer/clear_region` | Opaque-overlay reclaim a modal/popup runs every visible frame. |
| `layout/split/nested` | Nested app layout solve: a header/body/footer split, then a sidebar/content/aside split of the body. |

## Add a scenario

Edit [`crates/rstui-bench/src/scenarios.rs`](../crates/rstui-bench/src/scenarios.rs):

1. Write `fn name(bench: &Bench) -> Stats`.
2. Do **all** allocation and setup *outside* `bench.run(...)`. Measure **only**
   the hot operation inside the closure — setup in the closure pollutes the
   timing.
3. Use **only** `rstui-core`'s public API. This crate is a consumer of the
   published surface, exactly like a downstream app; never reach into another
   crate's internals.
4. Add a `("name/with/slashes", fn)` row to the `SCENARIOS` registry. Keep the
   name unique and `/`-segmented so a prefix selects a family.

`cargo test -p rstui-bench` runs every scenario once (1 warmup-free iteration)
and asserts a sane summary, so the benches can't bit-rot even though they don't
gate CI.

## Profiling (CPU and memory)

Always profile a `--release` build, and raise `RSTUI_BENCH_ITERS` for a longer,
steadier sample. Build the binary once, then point a profiler at it with a
scenario filter so you profile only the path you care about:

```sh
cargo build --release -p rstui-bench
```

The binary is then `./target/release/rstui-bench`; pass it a scenario substring
filter the same way `cargo xtask bench` does.

`rstui-core` is dependency-free pure logic, so heap allocation count and size is
the primary memory signal — there is nothing else to attribute. Watch
allocations in the hot scenarios (for example the `Vec` `Buffer::diff` returns
each frame).

### macOS

```sh
cargo install samply
cargo build --release -p rstui-bench
samply record ./target/release/rstui-bench buffer/diff
```

Apple Instruments, via either:

```sh
cargo instruments --release -p rstui-bench --template 'Time Profiler' -- buffer/diff
```

```sh
cargo build --release -p rstui-bench
xcrun xctrace record --template 'Time Profiler' --launch ./target/release/rstui-bench buffer/diff
```

`valgrind` does **not** work on macOS — this is part of why ADR 0005 chose a
dependency-free harness over `iai-callgrind` (which needs valgrind/callgrind).
For memory on macOS, lean on the allocation-count signal above and Instruments'
Allocations template.

### Linux

CPU, with `samply` or `cargo flamegraph`:

```sh
cargo install samply
cargo build --release -p rstui-bench
samply record ./target/release/rstui-bench buffer/diff
```

```sh
cargo install flamegraph
cargo flamegraph --release -p rstui-bench -- buffer/diff
```

Memory, with `valgrind`'s massif (allocation profile over time) or `heaptrack`:

```sh
cargo build --release -p rstui-bench
valgrind --tool=massif ./target/release/rstui-bench buffer/diff
```

```sh
cargo build --release -p rstui-bench
heaptrack ./target/release/rstui-bench buffer/diff
```

---

See [ADR 0005](adr/0005-benchmarking-and-profiling-strategy.md) for why this
harness is dependency-free and non-gating.
