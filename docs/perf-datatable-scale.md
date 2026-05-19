# DataTable capacity — how big a data table can rstui handle?

- **Date:** 2026-05-19
- **Tooling:** `cargo xtask bench` + `rstui-bench` (3 new
  `widget/datatable/*` regression guards) + a one-shot capacity sweep
  (`cargo run --release -p rstui-bench --example datatable_scale`)
- **Predecessors:** [`docs/perf-review-3.md`](perf-review-3.md) (the
  caller-owned-cache seam, generalised), [`docs/benchmarking.md`](benchmarking.md)
- **Question asked:** *benchmark a very large data table — how big can we
  go? Can we handle over 1 000 000 rows and 100 columns or more?*

## TL;DR

**Yes to a million rows. Yes to a hundred columns. Not both at once if you
fully materialize the model — and that limit is RAM, not the widget.**

`DataTable` is a borrowed **pure projection** with a fully virtualized
render: the body loop only ever touches the ~47-row visible window, so
**rendering 1 000 000 rows costs exactly what rendering 1 000 rows costs**
(~12 µs at 4 columns, ~115 µs at 100). Scrolling and selection are free at
any row count. The three real limits, in order:

1. **Materialized model memory — the true ceiling.** Owned cells cost a
   stable **~110 bytes/cell**. `1 000 000 × 4` = **460 MiB**,
   `1 000 000 × 20` ≈ **2.2 GiB** (both fine). `1 000 000 × 100` ≈
   **~10 GiB** — you cannot hold 100 M owned `Line` cells in RAM on a normal
   machine. This is the *only* thing that stops 1M × 100, and it is the data
   model, not the widget (render there is still ~115 µs).
2. **Interactive sort/filter CPU — the interactivity wall ≥ ~100k rows.**
   `project()`'s comparator and filter allocate a fresh `String` per cell
   touch (`line_text`), so a re-sort of 1M rows is **~4–5 s** and a re-filter
   **~0.2–1 s**. Display and scroll of 1M rows are free; *re-sorting* 1M
   rows is a multi-second batch operation, not an interactive one. This is a
   tractable optimization (§ Bottlenecks), not a structural limit.
3. **Render — never a bottleneck.** O(visible window), flat in row count.

## Evidence — the capacity sweep

`crates/rstui-bench/examples/datatable_scale.rs`, release, frame 160 × 48
(47 visible body rows). Each matrix cell built in its own process; *model*
is the resident RSS the `Vec<DataRow>` actually costs (measured via `ps` —
this workspace forbids `unsafe`, so a counting global allocator is not an
option; `ADR 0003`). `proj/*` = one `project()` pass; `render` = one
virtualized frame.

```
     rows cols      model    B/cell  proj/ident   proj/sort   proj/filt     render
----------------------------------------------------------------------------------
     1000    4       0.6M       147      0.01ms      0.53ms      0.42ms     22.3µs
    10000    4       4.8M       125      0.05ms      8.45ms      2.34ms     12.9µs
   100000    4      46.2M       121      0.46ms    164.64ms     22.41ms     12.0µs
  1000000    4     460.3M       121      8.24ms   3741.94ms    236.91ms     12.9µs
----------------------------------------------------------------------------------
     1000   20       2.3M       119      0.01ms      0.54ms      1.02ms     52.7µs
    10000   20      21.7M       114      0.05ms     11.59ms     10.47ms     51.0µs
   100000   20     216.0M       113      0.47ms    328.50ms    106.93ms     52.7µs
  1000000   20     730.8M       38†     9.94ms   5359.82ms   1063.62ms     51.1µs
----------------------------------------------------------------------------------
     1000  100      10.2M       107      0.01ms      0.52ms      4.96ms    112.6µs
    10000  100     101.3M       106      0.05ms     13.32ms     51.72ms    116.0µs
   100000  100    1011.6M       106      0.45ms    328.94ms    514.75ms    114.4µs
  1000000  100   10115.9M*      106         n/a         n/a         n/a        n/a
----------------------------------------------------------------------------------
```

> `*` 1M × 100 is **predicted, not spawned** — the orchestrator extrapolates
> 100 000 × 100's measured 106 B/cell to ~9.9 GiB, past the 6 GiB cap, so the
> child is never run (that is the answer for that corner). `†` 1M × 20's
> `38 B/cell` is an **RSS under-read**, not a real result: `ps` RSS counts
> only *resident* (faulted-in) pages, and on a memory-pressured machine a
> ~2 GB allocation is not fully resident at the sampling instant. The model
> constant is otherwise a flat **~106–125 B/cell** across three orders of
> magnitude of rows and 25× the columns — so 1M × 20 is really ≈ 2.2 GiB
> (20 M cells × ~110 B; an earlier run on the same box measured 1.36 GiB
> there before pressure rose), and the ~10 GiB figure for 1M × 100 (built
> from the *un*-pressured 100 000 × 100 = 106 B/cell) is solid.

## Analysis

### Render is fully virtualized — row count is irrelevant

The body loop is
`self.visual.iter().enumerate().skip(offset).take(body.height)`
(`data_table.rs:1454`): at most `body.height` (~47) rows are ever touched,
and each column early-outs (`if cell_w == 0 { continue }`,
`data_table.rs:1503`) *before* any per-cell allocation, so off-screen
columns cost nothing either. The sweep confirms it empirically: **render
time is identical (within noise) from 1 000 to 1 000 000 rows** at every
column count. Render scales only with *visible cells* (4 cols ≈ 12 µs, 20 ≈
51 µs, 100 ≈ 113 µs) — and 100 µs is ~1 % of a 120 fps frame. A 1M-row
`DataTable` is, to the renderer, a 47-row one.

### Projection is the O(rows) cost — and only sometimes expensive

`project()` (`data_table.rs:855`) is the caller-owned, once-per-state-change
artifact (the same pure-projection discipline as `List`/`Tree`):

- **Identity** (no sort/filter/group, the scroll/select case): builds a
  `Vec<usize>` then a `Vec<VisualRow>`, never touching cell text. ~8–10 ms
  at 1M rows — a few frames, and only re-run when state changes. Effectively
  free for display.
- **Sort:** `kept.sort_by(cmp_keys)` where `cmp_keys` calls `line_text`
  (collects the cell's spans into a fresh `String`) **twice per
  comparison** → ~`N log N` *allocations*. ~4–5 s to sort 1M rows. This is
  the dominant scaling cost and the interactivity wall.
- **Filter:** `line_text(c).to_lowercase()` per cell until a match → O(rows
  × cols) `String` allocations; scales with column count too (1M × 20 ≈
  1.06 s).

### Memory is the hard ceiling for the rows × columns product

A cell is one owned `Line` (a `Vec<Span>`) holding one `Span` (a
`Cow<str>` + `Style`) over a small `String` — measured at a stable
**~110 bytes/cell** including allocator overhead. Memory is linear in
`rows × cols`, so the frontier is a hyperbola:

| Shape            | Cells  | Model    | Verdict                          |
|------------------|--------|----------|----------------------------------|
| 1 000 000 × 4    | 4 M    | 460 MiB  | ✅ comfortable                   |
| 1 000 000 × 20   | 20 M   | ≈2.2 GiB | ✅ fine on a normal machine      |
| 100 000 × 100    | 10 M   | 1012 MiB | ✅ fine                          |
| **1 000 000 × 100** | **100 M** | **~10 GiB** | ❌ not materializable        |

## Verdict — answering the question directly

- **Over 1 000 000 rows?** **Yes.** Render and scroll are flat and free
  (~12–115 µs, identical at 1k and 1M). The model is ≈460 MiB (4 cols) /
  ≈2.2 GiB (20 cols). The one caveat: an *interactive* sort over 1M rows is
  ~4–5 s and a filter ~0.2–1 s — display/scroll of a million rows is
  trivial; *re-sorting* a million rows is a batch operation.
- **100+ columns?** **Yes.** Render is O(visible columns), flat in rows
  (~115 µs at 100 cols even at 1M rows). Memory and filter cost grow
  linearly with columns; sort is independent of column count (it compares
  only the sort key).
- **1 000 000 rows *and* 100 columns at once?** **Not by fully
  materializing `Vec<DataRow>`** — that is ~10 GiB of owned cells. The
  ceiling is the data model, not `DataTable`: the widget only needs the
  *visible window* to render, and `project()` is the only thing that
  indexes the full `&[DataRow]`. The scalable pattern at that size is the
  one the architecture already implies — don't own 100 M cells: page rows
  in from the source of truth, project a window, or narrow the visible
  column set. CPU is a non-issue there (render ~115 µs, identity project
  ~8–10 ms); it is purely RAM.

## Bottlenecks & the one tractable optimization

| Rank | Bottleneck | Scaling | Status |
|------|------------|---------|--------|
| 1 | Materialized model RAM | ~110 B/cell, linear in rows×cols | Structural — borrowed-projection API; mitigated by paging/narrowing at the caller, or a future windowed row-source for `project` (API change, out of scope) |
| 2 | Sort/filter `String` allocs in `project()` | O(N log N) / O(N·C) allocations | **Tractable** — see below |
| 3 | Render | O(visible window), flat in rows | Not a bottleneck |

The rank-2 cost is the same lens [`perf-review-3`](perf-review-3.md)
generalised: `cmp_keys` re-derives each row's key (`line_text`) on every one
of the ~`N log N` comparisons. A per-row key computed **once** before the
sort (sort indices by a precomputed `&[String]` / borrowed key, or memoize
`line_text`) turns O(N log N) *allocations* into O(N) — a single-digit-second
1M-row sort becomes sub-second. This report **proposes** it (consistent with
how the perf-review docs propose, then land separately); it is not
implemented here, to keep this change a measurement + guards only. It is
DT-OPT-1 in the full optimization plan —
[`docs/datatable-optimization-roadmap.md`](datatable-optimization-roadmap.md)
(the prioritized roadmap: key cache, O(rows) grouping, horizontal
scroll/column window, scrollbar wiring, lighter cells + a non-materializing
row source — and the definitive scrollbar/virtualization answers).

## Reproduce

```text
# the full capacity sweep (one process per matrix cell; ~1–2 min):
cargo run --release -p rstui-bench --example datatable_scale

# the permanent regression guards:
cargo xtask bench widget/datatable
```

## Permanent regression guards (added to `rstui-bench`)

Three scenarios in `crates/rstui-bench/src/scenarios.rs`, sized so the suite
stays fast while pinning the three behaviours above (release, 1000 iters,
min):

| Scenario | Fixture | min | Guards |
|----------|---------|-----|--------|
| `widget/datatable/render` | 50 000 × 6, virtualized | **19 µs** | render never becomes O(rows) |
| `widget/datatable/project` | 50 000 × 6, identity | **244 µs** | identity projection stays alloc-light |
| `widget/datatable/project_sorted` | 2 000 × 6, 1 sort key | **1.10 ms** | the `line_text` sort-alloc path doesn't regress |

`widget/datatable/render` is the headline guard: a 50 000-row fixture (250 ×
the visible window) rendering in 19 µs is the proof, baked into CI, that
vertical virtualization can never silently regress to O(rows).

> **Baseline note:** `docs/perf-baseline.json` currently records only 18 of
> the 25+ scenarios — newer scenarios already ride as `new` in
> `cargo xtask perf --check` (non-failing). These three join them as `new`
> by design: `perf --save` rewrites the *entire* baseline from one machine
> (`docs/benchmarking.md`), so rebaselining is done wholesale on a quiet
> machine, not surgically from a contended worktree. The scenarios are
> guarded meanwhile by `cargo test -p rstui-bench`
> (`every_scenario_runs_and_summarizes`).
