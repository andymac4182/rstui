# DataTable optimization roadmap — lighter weight, more rows & columns

- **Date:** 2026-05-19
- **Tooling:** code review (`crates/rstui-widgets/src/{data_table,table,scrollbar}.rs`,
  `crates/rstui-core/src/scroll.rs`) + the measured
  [`docs/perf-datatable-scale.md`](perf-datatable-scale.md) capacity sweep
- **Predecessor:** [`docs/perf-datatable-scale.md`](perf-datatable-scale.md)
  (how big it goes today: 1M rows render flat; ~110 B/cell is the ceiling;
  sort is the interactivity wall)
- **Questions asked:** *what optimizations make data tables lighter weight and
  able to support larger amounts of rows and columns? Do we need to add
  virtualisation or similar? Do we have vertical and horizontal scrollbars?*

## TL;DR — the three questions, answered

1. **Do we have vertical/horizontal scrollbars for data tables?**
   - **Vertical scroll: yes** — `DataTableState.vertical: ScrollState`
     (`data_table.rs:519`) with a full, total scroll API
     (`scroll_by`/`scroll_to_end`/`reveal_selected`/`clamp`/
     `on_content_change`, `:741‑790`); the render is virtualized off
     `state.offset()`.
   - **A vertical scroll*bar*: the widget exists but is wired nowhere for
     data tables.** `Scrollbar` (`scrollbar.rs`) is a complete pure-projection
     widget supporting **both axes** (`VerticalRight/Left`,
     `HorizontalBottom/Top`). `List`/`Paragraph` compose it (e.g.
     kitchen-sink `logs.rs`), but `data_grid.rs` renders `DataTable` with
     **no scrollbar at all** — there is scroll *state* but no on-screen
     indicator.
   - **Horizontal scroll: none.** Neither `DataTable` nor `Table` has any
     column/x offset. Columns are always solved by
     `Layout::horizontal(constraints)` across the **full inner width**
     (`data_table.rs:1162‑1168`), so 100 `Fill(1)` columns in a 160-col frame
     become ~1 char each — they *crush*, they don't *scroll*. This, not CPU,
     is why "100+ columns" isn't usable today.

2. **Do we need to add virtualisation?**
   - **Vertical render virtualization: already done, and excellent — do not
     re-add it.** The body loop is `…skip(offset).take(body.height)`
     (`data_table.rs:1454`); the sweep measured render *flat* at 12–115 µs
     from 1 000 to 1 000 000 rows.
   - **Column virtualization: not present, but nearly free to get.** Render
     already early-outs every off-screen column (`if cell_w == 0 { continue }`
     *before* any cell work, `data_table.rs:1503`). Add a horizontal offset
     (DT-OPT-3) and column virtualization falls out almost for nothing.
   - **Data-model virtualization (lazy rows): this is the one place "more
     virtualization" is genuinely needed.** `project()` indexes the full
     `&[DataRow]`, so the caller must materialize every row
     (~110 B/cell → 1M × 100 ≈ 10 GiB). A windowed/lazy row source
     (DT-OPT-5b) is the only true lever for *unbounded* data.

3. **What optimizations?** Ranked roadmap below. The headline: the framework
   is already 120 fps-ready for *display* of huge grids; the work is
   **(a)** kill the per-comparison `String` alloc in `project()`,
   **(b)** add a column window + scrollbars so wide grids are usable, and
   **(c)** offer a non-materializing row source for unbounded data — all
   additive, in the caller-owned-pure-projection idiom this codebase already
   uses (ADR 0012/0025).

## What you have today (code-evidenced)

| Concern | State | Evidence |
|---|---|---|
| Vertical render virtualization | ✅ done, flat to 1M rows | `data_table.rs:1454`; sweep |
| Vertical scroll state | ✅ full total API | `data_table.rs:741‑790`, `scroll.rs` |
| Vertical scrollbar | ⚠️ widget exists, **unwired** for tables | `scrollbar.rs`; `data_grid.rs` has none |
| Horizontal scroll | ❌ none (columns crush to fit) | `data_table.rs:1162‑1168`, `table.rs:390` |
| Horizontal scrollbar | ⚠️ widget supports it, nothing feeds it | `scrollbar.rs` `HorizontalBottom/Top` |
| Column virtualization | ❌ none, but ~free given an x-offset | `data_table.rs:1503` early-out |
| Data model | ❌ caller must materialize all rows | `project()` indexes `rows[i]` |
| Sort projection | ⚠️ ~4–5 s at 1M (String alloc/cmp) | `data_table.rs:872‑885`; sweep |
| Group projection | ⚠️ O(rows × distinct groups) + String/row | `data_table.rs:893‑899` |

`Table` (the simpler widget) copes with many columns via
`TableColumnFit::{Manual,Proportional,Balanced}` + `wrap_cells`
(`table.rs:49,390`) — fit/wrap, still **no scroll**. The recommendations
below target `DataTable` (the real data grid); `Table` benefits from
DT-OPT-5a (lighter cells) only.

## The roadmap (prioritized; report-only — propose now, land as slices)

Effort: S ≤ ½ day · M ≈ 1–2 days · L ≈ multi-day. Each is additive and
gate-green-by-construction (byte-identical default path), the discipline of
`docs/perf-review-3.md`.

### DT-OPT-1 — Projection key cache (sort/filter de-allocation) · Impact ★★★ · Effort M · Risk low

**Problem.** `project()`'s comparator allocates a fresh `String` *twice per
comparison* via `line_text` (`data_table.rs:872‑885`) → ~`N log N`
allocations; the filter does `line_text(c).to_lowercase()` per cell
(`:857‑866`). Measured: **~4–5 s to sort 1M rows**, ~0.2–1 s to filter.

**Fix.** A caller-owned, `(rows-identity, sort/filter config)`-keyed cache of
each row's precomputed comparison/filter key (its joined lowercased text),
computed **once** before the sort and indexed during it — exactly the
`LineCache`/`MarkdownCache`/`ConversationCache`/`DiagramCache` seam this
codebase has shipped four times (ADR 0025). Sort becomes O(`N log N`)
*comparisons* over `&str` with **O(N)** allocations; a multi-second 1M-row
re-sort drops to sub-second. Default path stays byte-identical (same
ordering); the cache is opt-in like `Markdown::cache(&c)`.

> This is the single highest-leverage, lowest-risk change and the one
> `perf-datatable-scale.md` already flagged. Do it first.

### DT-OPT-2 — O(rows) grouping + interned group keys · Impact ★★ · Effort S · Risk low

**Problem.** Grouping buckets with `buckets.iter_mut().find(|(k,_)| *k==key)`
**per row** (`data_table.rs:893‑899`) → O(rows × distinct groups); plus a
`String` per row in `group_key` and a `String` per group in
`VisualRow::Group{ key: String }`.

**Fix.** Bucket via a `HashMap<&str/u64, Vec<usize>>` keyed off the
DT-OPT-1 key cache (O(rows)); carry an interned/`Rc<str>` group key so the
header doesn't re-allocate. Internal, byte-identical output. Naturally
composes with DT-OPT-1's cache (shared key source).

### DT-OPT-3 — Horizontal scroll + column window · Impact ★★★ · Effort M · Risk med

**Problem.** No way to *read* >~15 columns: they crush to sub-char width.

**Fix.** Add `horizontal: ScrollState` to `DataTableState` (sibling of
`vertical`, same proven one-axis primitive — `scroll.rs` §"one axis on
purpose"), measuring in *column* units. Teach `geometry()` to start the
`Layout::horizontal` solve at the offset column (and offset `rect.x`). The
render's existing `cell_w == 0 { continue }` (`data_table.rs:1503`) then
*is* column virtualization — only on-screen columns cost anything, for free.
Header band is already a separate rect, so it stays pinned automatically;
optional follow-on: freeze N leading columns (don't offset the first N
rects). `hit`/`cell_rect` get the same offset. Total/additive; absent
horizontal state = today's behaviour byte-for-byte.

### DT-OPT-4 — Wire vertical + horizontal scrollbars · Impact ★★ · Effort S · Risk low

**Problem.** The `Scrollbar` widget (both axes, pure projection) is never
fed by a data table; `data_grid.rs` shows no indicator.

**Fix.** `DataTable` already exposes `vertical() -> ScrollState`; add
`horizontal()` once DT-OPT-3 lands. Compose a `VerticalRight` scrollbar
(content = `visual.len()`, viewport = body height, position = offset) and a
`HorizontalBottom` one (content = column count, viewport = visible columns)
in the caller — the proven `logs.rs` pattern — and wire it in kitchen-sink
`data_grid.rs` as the worked reference + a `docs/composition.md` note. Pure
projection: the widget still draws no chrome; the app composes it.

### DT-OPT-5 — Lighter cells (a) & non-materializing row source (b) · Impact ★★★ · Effort a:M b:L · Risk a:low b:med

**Problem.** A cell is an owned `Line` (`Vec<Span>` + `Style`) ≈ **~110
B/cell**; the caller must own *every* row because `project()` indexes
`rows[i]`. 1M × 100 ≈ 10 GiB — the hard ceiling.

**Fix (a), lighter materialized — do with DT-OPT-1.** A text-cell fast path:
a `DataRow` variant whose cells are `Cow<'a,str>` (no per-cell `Vec<Span>`/
`Style`) for the common plain-text grid. Large constant-factor memory cut
(drops the per-cell `Vec` header + Style), additive next to the styled
`Line` path. Benefits `Table` too.

**Fix (b), the unbounded-data answer — strategic.** A borrowed
`RowSource` seam: `project()`/render pull a row by index from a caller
trait (`fn row(&self, i: usize) -> DataRowRef` over a columnar/Arrow-like
store or a generator) instead of requiring a fully-materialized
`&[DataRow]`. Then 1M × 100 *never exists* as 100 M owned cells — only the
projected window is realized. Gate it behind the new trait so the existing
`&[DataRow]` constructor stays byte-identical; this is the only place the
answer to "add virtualisation" is **yes — virtualize the data model**.

### Smaller wins (fold into the above)

- **Per-frame cell alloc:** render computes `let text = row.cell(ci)
  .map(line_text)` for *every visible cell every frame*
  (`data_table.rs:1519`) but only uses it in the Checkbox/Switch branch.
  Hoist it into those branches → removes ~`visible_rows × visible_cols`
  `String` allocs per frame on text grids. (S, fold into DT-OPT-3.)
- **Filter case-fold without alloc:** replace
  `line_text(c).to_lowercase().contains(needle)` with an allocation-free
  ASCII/Unicode case-insensitive search over the cached key (DT-OPT-1). (S.)

## Conflict-free batch plan & recommended order

All five touch mostly disjoint regions; sequence by leverage and dependency:

1. **DT-OPT-1** (key cache) — biggest win, unblocks 2. *Independent.*
2. **DT-OPT-2** (O(rows) grouping) — consumes DT-OPT-1's key cache.
3. **DT-OPT-5a** (lighter text cells) — independent of 1/2; pairs naturally
   with the key cache (key derivation already centralised).
4. **DT-OPT-3** (horizontal scroll/column window) — independent
   `geometry()`/state change; the high-value *usability* fix for wide grids.
5. **DT-OPT-4** (scrollbar wiring) — depends on DT-OPT-3 for `horizontal()`;
   otherwise a small composition slice.
6. **DT-OPT-5b** (row source) — strategic, last; large, gated behind a new
   trait, byte-identical default.

Each lands as its own locked slice with a `widget/datatable/*` bench guard
(the three from `perf-datatable-scale.md` already pin render & the sort
path, so DT-OPT-1's win is measurable as a regression-proof delta).

## Verdict

- **Scrollbars:** vertical scroll *state* and a both-axis `Scrollbar` widget
  exist; **no horizontal scroll and no scrollbar is wired to any data
  table** — DT-OPT-3 + DT-OPT-4 close this.
- **Virtualization:** vertical render virtualization is **already done** —
  don't add it. Column virtualization is ~free once DT-OPT-3 adds an
  x-offset. The only genuinely missing virtualization is of the **data
  model** (DT-OPT-5b), the lever for truly unbounded rows × columns.
- **Lighter weight / bigger grids:** DT-OPT-1 (sub-second 1M sort) and
  DT-OPT-5a (constant-factor memory cut) are the immediate wins; DT-OPT-5b
  is the strategic ceiling-lift. None require rewriting the engine — every
  item is the additive caller-owned-projection seam this codebase already
  proves.
