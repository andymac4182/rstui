# Performance review: reducing copies, tightening data lifecycles

A codebase-wide audit of every crate, widget, and core struct, focused on
**reducing copies/clones, improving data lifecycles, and cutting memory/CPU**.

Method: 15 parallel read-only audits (one per non-overlapping file group),
each producing `file:line`-anchored findings with severity, cost, a concrete
fix, and an API-breaking flag. Raw reports: `target/perf-audit/*.md`
(gitignored). This document is the synthesis: the cross-cutting thesis, a
leverage-ranked plan, conflict-free implementation batches, and the
validation path for each change.

> **Landed on `origin/main`** (measured, gate-green, byte-identical unless
> noted):
> - **Batch B** `Buffer::diff` flat-slice rewrite + `reset`→`slice::fill`:
>   the documented #1 hot path, **3.3–8.4× faster** (`diff/identical`
>   40.1→12.3 µs, `resized` 45.2→5.4 µs).
> - **Batch A** crossterm one reused `Vec<u8>` + single `write_all`/frame:
>   **~10k–40k locked syscalls/frame → 1** (composed with `ColorLevel`).
> - **Batch C (CR-05)** `Layout::split_into` + scratch-free `solve`:
>   `layout/split/nested` **416 → 83 ns (~5×)**.
> - **Batch B2 (CR-08/03/06/04)** `Buffer::row_slice_mut`/`Rect::rows` +
>   row-slice `set_str`/`set_style`/`clear_region`/`resize`:
>   `clear_region` **3.9 → 0.63 µs (~6.5×)**, `set_str` faster.
> - **Batch D** `Line::render` Left-align width-scan skip + empty-style
>   row-fill guard + `Span::width` ASCII fast path.
> - **Batch H** plugin-host `read_frame`/encoders/`Log` zero-extra-copy
>   frame path.
> - **Batch E partial** `TextEdit` O(1) `len()` cache + single-walk
>   `delete_backward` (CM-2/CM-4); `SelectionSpan` frame-scoped projector
>   (CM-1).
> - **Batch F** runtime RT-07: inline-1 `Effects` enum — single-effect
>   `Cmd` (the common case + every test) allocates zero. (RT-01 render
>   gating + RT-05 coalescing already landed via a concurrent stream;
>   RT-02 audit-noted low-value, RT-03 cadence-risk — both skipped.)
> - **Batch I partial** acp-client APP-4: `tool_call` id→index map,
>   per-frame O(toolEntries×toolCalls) → O(1).
> - **Batch G partial** per-frame allocation removed:
>   `calendar` (W1-01), `stepper` (W5-STEP-2), `line_number_gutter`
>   (GUT-1), `avatar` (W1-03), **`table` T1** (wrapped cell `Line`
>   deep-cloned twice/frame → once), **`paragraph` PG-1** (`compose_rows`
>   early-exits at the visible window — caps Toast/DescriptionList/
>   Table-wrap), **the entire `block.clone()` cluster ×8** —
>   `Block::render_ref(&self)` added + `data_table`/`diff`/`help_overlay`/
>   `line_number_gutter`/`mermaid`/`markdown`/`stepper` borrow it,
>   `date_picker` moves the block into `Calendar` (DP-1), **`editor`
>   EDIT-1** (skip flat-index extmark scans when no extmarks).
> - **Batch I partial** acp-client: APP-4 `tool_call` O(n²)→O(1);
>   **UI-3** `footer_segments` borrows; **APP-3** diagnostic `log` capped.
> - **Batch G long tail** also done, all byte-identical no-alloc stamping:
>   `pagination` PA-1, `breadcrumb` W1-02 (drop `Vec<Crumb>`+enum),
>   `help_overlay` HELP-1 (`Kbd::cluster_width`, no per-entry clone),
>   `grid` GRID-1 (`cell()` solves one band, not the whole grid),
>   `gauge` GAUGE-1 (no default-label `format!`).
> - **Batch I**: APP-1 transcript cap (generous, sentinel).
> - **Batch J COMPLETE** (BN01+BN02+BN03 — the audit's bench-gap closed):
>   `edit/textarea/insert`, `selection/extract`,
>   `widget/{list,table,tree,paragraph,markdown}/render`, and
>   `runtime/frame/{idle,changed}` (the end-to-end frame via the public
>   `Harness`). **These produced the decisive empirical ranking:**
>
> | scenario, min (160×48) | |
> |---|---|
> | `widget/tree/render` | ~19 µs (clean windowed exemplar) |
> | `widget/table/render` | ~32 µs |
> | `widget/list/render` | ~34 µs |
> | `widget/paragraph/render` | ~59 µs (post-PG-1) |
> | `runtime/frame/idle` ≈ `runtime/frame/changed` | **~83 µs** |
> | **`widget/markdown/render`** | **~1.49 ms** |
>
> Two findings drive everything remaining: (1) Markdown render is **~100×
> the next-heaviest widget and ~120× a full-frame `Buffer::diff`** (it
> re-parses the whole CommonMark source every render). (2) An *idle*
> re-render costs the **same** as a changed one (~83 µs) — the per-frame
> `view` re-projection dominates, not the (now ~12 µs) diff. So the only
> high-leverage work left is **eliminating per-frame re-derivation**:
> caller-owned caches/windowing. MD-1 is #1 by two orders of magnitude.
>
> **MD-1 design note (a real wrinkle, not a quick slice):** `blocks_into`
> (the parse) takes `focused_link` *and* `theme` as inputs — the parse is
> **not** purely source-dependent. A correct caller-owned `MarkdownDoc`
> must key on `(source, focused_link, theme)` and re-derive on link-focus
> change; a naive "parse once on source" cache silently renders **stale
> link highlighting** — a correctness bug the static markdown snapshots
> (which never toggle focus) would not catch. This is exactly why MD-1 is
> an ADR-gated architectural change, not a byte-identical one-shot.
>
> **~31 measured, gate-green slices total** (the byte-identical set + the
> complete additive bench + APP-1's generous cap + **DRV-1** and **PG-2**,
> landed `e0ca4ad`/`3a2a9a4`). **The remainder is now empirically ranked,
> not just risk-classified** — and each is gated by a *concrete,
> code-grounded barrier the `cargo xtask ci` gate cannot clear*, not by
> effort:
> - **PG-2 — DONE (`3a2a9a4`).** Re-classified out of Tier-2: a count-only
>   path needed *no* API change. `Paragraph::line_count` now calls
>   `count_rows` (a line-for-line transliteration of
>   `compose_rows`/`wrap_cells`/`flush_row` with the per-row cell `Vec`
>   replaced by a `usize`); an exhaustive matrix test (15 texts × 3 wraps ×
>   10 widths) gate-enforces `== compose_rows(.., usize::MAX).len()`
>   exactly. Toast/DescriptionList no longer compose twice/frame.
> - **DRV-1 — DONE (`e0ca4ad`).** `summarize_update` matches the typed
>   `SessionUpdate` enum directly for exactly `ContentBlock::Text`
>   `AgentMessageChunk`/`AgentThoughtChunk` (the per-token hot path);
>   every other content/variant **falls through to the unchanged
>   `serde_json` path** — safe by construction, behaviour-identical for
>   the replaced case. **DRV-2 — intentionally NOT converted:**
>   `describe_permission` is a *cold* path (human-gated permission
>   prompts, not per-token) and its doc comment documents the JSON
>   indirection as a deliberate schema-resilience choice; a typed rewrite
>   is a behaviour-risk change against documented design intent for zero
>   measurable hot-path benefit. The analysis *is* the deliverable: don't.
> - **Tier-2 architectural — headed by MD-1** (`Markdown` cache:
>   ~1.49 ms→~visible; #1 by 100×), then `Diff`/`Mermaid` re-parse,
>   borrowed `List`/`Table`/`Stepper`/`Tabs` constructors, plugin-host
>   PROTO-3 `Cow` payload, acp-client UI-1/UI-2 per-`Entry` memo.
>   **MD-1's concrete barrier:** it adds a *public* `MarkdownDoc` type
>   (hard to reverse — semver surface) **and** the gate is provably
>   insufficient for its known failure mode — `blocks_into` keys on
>   `focused_link`+`theme`, so a stale-link-highlight regression only
>   manifests when focus *changes between frames* against a cached doc, a
>   stateful interaction the static markdown snapshots (fixed
>   `focused_link`) never exercise. Plan prescribes an ADR note.
> - **`List`-API-coupled**: **SB-1/MENU-1/CP-1** build all-N rows then
>   hand the whole `Vec` to `List`, which clips internally. Pre-windowing
>   in the widget would duplicate `List`'s scroll/selection/highlight-bar
>   index math (off-by-one risk the snapshots may not fully cover);
>   correctly sequenced *after* the Tier-2 `List` borrowed/windowed
>   constructor (LIST-1), not doable in isolation safely.
> - **Design decision the gate cannot adjudicate**: `table` T3/T5
>   (col-count / proportional widths from the visible window vs all rows).
>   Both scans are inherently "all rows" — any bound *is* an output
>   change, so there is no behaviour-preserving subset. Worse, windowing
>   `col_count` makes the **column count change mid-scroll**; whether that
>   jitter is acceptable is a UX/design call the snapshot gate cannot
>   make — it records whatever is chosen, right or wrong. acp-client
>   **APP-1** (transcript cap — visible history truncation) is the same
>   class.
> - **Highest silent-corruption surface**: Batch E **CM-3** (`TextArea`
>   `line_lens` parallel cache across ~10 distinct `lines` mutation sites;
>   a desync on a rare op interleaving ships silent editing corruption,
>   and the target cost is already viewport/line-bounded in practice —
>   the `Editor` windows to visible rows).
> - **Additive infra**: Batch J bench scenarios (`view→diff→flush`,
>   per-widget, edit) + alloc counter — substantial, drives the runtime
>   loop.
> - **Accepted as-is**: gauge GAUGE-1 (P3 — a byte-identical no-`format!`
>   fix needs replicating f64 `Display` incl. inf/NaN; not worth the risk
>   for ≤4 bytes/frame; the audit says accept it).
>
> Landing is **serial from one isolated worktree**
> (fetch→rebase `origin/main`→`cargo xtask ci`→FF push) — the
> parallel-agents-in-one-shared-worktree approach corrupts state in this
> multi-stream repo and must not be retried.
>
> Also assessed & **rejected — not viable**: replacing `tokio` with
> `smol`. `tokio` is pulled transitively by `sacp`→`rmcp` (the ACP
> protocol stack is tokio-native), is already absent from the sync
> core/widgets/runtime path (it's `optional`, ADR 0011), and is not on the
> per-frame hot path — swapping would add a *second* runtime, bigger and
> slower. Zero leverage on per-frame performance.

Severity = how often the cost is paid: **P0** every frame / per-cell ·
**P1** per widget render · **P2** per event/interaction · **P3** cold.
Leverage = severity × frequency × breadth ÷ effort.

Constraints every fix must respect (verified against the ADRs):

- **Pure projection, no retained widget tree** (ADR 0012): a widget may not
  cache across frames in interior state. Caching lives in *caller-owned*
  model state (the `ScrollState`/`Input`/`Editor` seam) — "caller-side
  windowing is the pure-projection answer to virtualization"
  (`docs/composition.md`).
- **`rstui-core` is dependency-free and `unsafe` is `forbid`** (workspace
  lints, ADR 0001/0003): std only — `Cow`/`Box<str>`/`Rc<str>`/slices, no
  `smallvec`/`compact_str` in core, no uninitialized-memory tricks.
- **Validation is the slow loop** (ADR 0005): `cargo xtask bench` (`min` is
  the signal), non-gating. ADR 0005 follow-up explicitly sanctions adding
  widget/runtime-loop scenarios — see Batch J.

---

## The headline

A documented benchmark fact frames everything: `buffer/diff/identical` (a
**zero-change idle frame**) costs ~39 µs — *the same as a full repaint*
(`docs/benchmarking.md`). An idle screen pays nearly full price. That single
number is explained by the two root causes below; the rest of the audit is
the same two patterns recurring in 56 widgets and two apps.

Authoritative struct sizes (measured, not estimated — settles two findings):

| Type | size | note |
|---|---|---|
| `Color` | 4 B | |
| `Option<Color>` | **4 B** | niche-filled — *identical* to `Color` |
| `Style` | 12 B | 4+4+2+2, zero padding — already minimal |
| `Cell` | **16 B** | `char` forces align 4 (14 → padded to 16) |
| double-buffer 160×48 | 240 KB | diff scans 122 KB/frame, 4 cells/cache-line |

→ **Rejected:** collapsing `Option<Color>`→`Color::Inherit` (core-style P1-1).
It saves **0 bytes** (niche optimization already applies) for an API break.
→ **Re-sequenced:** packing `Cell` 16→8 B (CR-07) is a real 2× memory/cache
win but API-breaking + truecolor-encoding work — deferred behind the
non-breaking index-math fixes that capture a similar magnitude.

---

## Root cause 1 — the per-cell `Position`↔`index_of` round trip (`rstui-core`)

`Rect::positions()` (`geometry.rs:272`) synthesizes a fresh `Position{x,y}`
per cell; every `Buffer` accessor immediately reparses it back to a flat
index via `index_of` (`buffer.rs:130`) — running `Rect::contains` (4
comparisons) + a multiply **per cell**, including in bulk paths where `p`
came from `self.area.positions()` so the bounds check *provably cannot
fail*. `Buffer::diff` pays it **twice per cell** (`self.get` + `previous.get`)
even in the equal-area branch where both `Vec<Cell>`s are the same length in
identical row-major order.

This one value-shape decision is the root of: `diff` (CR-01), `set_str`
(CR-03), `resize` (CR-04), `set_style`/`clear_region` (CR-06), `selected_text`
(CM-9), `Line::render` double char-scan (core-style P0-2), and the per-glyph
`set_cell` loops in dozens of widgets (table T2, tree TR2, `status_bar`,
`slider`, …).

**Enabling fix (non-breaking, additive): a row-slice primitive.**
`Buffer::row_slice_mut(y, x_range) -> &mut [Cell]` (+ `Rect::rows()`). Then
`diff` becomes a flat-slice `zip` (one `Cell` compare per cell over
contiguous memory, no `index_of`, autovectorizable), and `set_str` /
`set_style` / `clear_region` / `resize` each become *one* `index_of` per row
+ a slice write. Unlocks ~8 S-effort fixes and directly targets the
documented ~40 µs.

---

## Root cause 2 — "build the whole dataset every frame, then clip"

The pure-projection model is correct, but many widgets and both apps
re-derive *all* content for the whole dataset every frame and then
`skip().take()` the visible window — at 8–60 fps:

- **Re-parse every frame:** `Markdown` (MD-1), `Diff` (DIFF-1/2/3), `Mermaid`
  (MM-1/2) re-run their full parser+layout each frame and discard ~94%.
- **Re-wrap every frame:** `Paragraph::compose_rows` (PG-1) materializes every
  glyph of the whole document as `Vec<(char,Style)>`; `line_count` does it a
  *second* time just for `.len()` (PG-2) — so `Toast`/`DescriptionList`
  compose twice/frame (W5-TOAST-1, DESC-1).
- **Re-collect every frame:** `Table` rebuilds `Vec<Row>`+`Vec<Line>` and
  clones every wrapped cell *twice* (T1); `Menu`/`Sidebar`/`CommandPalette`
  build all-N item Vecs then clip (MENU-1, SB-1, CP-1); `table` scans **all**
  rows for `col_count`/proportional widths (T5/T3) — `tree.rs` is the
  exemplar that windows correctly.
- **Apps:** kitchen-sink Logs rebuilds+`format!`s a 600-line history every
  frame for ~30 visible rows (~1200 allocs/frame, KS01); the ACP client
  re-parses the *entire* Markdown transcript and word-wraps it *twice* every
  frame for the whole session (UI-1/UI-2), with O(toolEntries×toolCalls)
  lookups (APP-4) and unbounded transcript/log growth (APP-1/APP-3).

**Fix, two ADR-0012-compliant tiers:**
- **(a) Caller-side windowing** — only build `[scroll, scroll+height)`.
  Cheap, non-breaking, often S. `Paragraph` PG-1's early-exit and `tree.rs`
  are the exemplars.
- **(b) Caller-owned cached layout model** — `MarkdownDoc`/`ParaLayout`/
  `MermaidLayout`/`DiffModel` built once in `update` when source changes, the
  widget borrows it. Mirrors the existing `ScrollView`/`Input`/`Editor`
  seam — fully pure-projection-compliant. API-additive, L.

---

## Independent high-value findings

- **RT-01 (runtime, P0):** `render()` is called unconditionally after every
  input/tick/drain even when nothing changed — a full `view` re-projection +
  full-buffer `diff` every idle tick. Sits *above* root cause 1 (a perfect
  diff is still wasted if re-run on no change). Gate on `produced || resize`.
  **Biggest idle-CPU win.**
- **CT-1 (crossterm, P0):** no reusable byte buffer between backend and OS —
  every escape fragment is a separate mutex-locked `write_all` on unbuffered
  `io::Stdout` (verified against crossterm 0.29 source); a frame is
  10k–40k `write(2)`-candidate syscalls, not one. Add a reused `Vec<u8>`,
  one `write_all`+`flush` per frame. **S, non-breaking, single largest
  flush-path win.** + CT-3: per-channel SGR diff (don't re-emit unchanged bg).
- **core-io P1-2:** `Terminal::swap_buffers` unconditionally `reset()`s the
  whole back buffer every frame — O(cells) regardless of change. Make it a
  single `slice::fill` (or dirty-region). Pairs with RT-01.
- **CR-05 (`Layout::split`, P1, cross-widget):** 6+ scratch `Vec`s per
  container per frame for inputs constant frame-to-frame; the true
  cross-widget core hotspot (table T4, grid GRID-1, form FORM-1, flow
  FLOW-1, split_pane W5-SPLIT-1). `widgets/lib.rs` has *no* shared helpers —
  the shared hotspots are all in `rstui-core`. Add `split_into(&mut Vec)` +
  stack arrays for small N + optional memoization.
- **CM-2/CM-3 (core editing, P1):** `TextEdit::len()` / `TextArea::
  line_char_len()` are O(n) UTF-8 re-scans on the per-frame projection path
  *and* per keystroke. Cache char counts incrementally (private fields,
  non-breaking).
- **plugin-host PROTO-1/2/3:** a capability round-trip allocates 6–8 heap
  `Vec`s, none reused; `read_frame` double-allocates+zero-fills+copies
  (PROTO-1, S non-breaking). `Frame::payload: Vec<u8>` is the structural
  root (PROTO-3, L API-breaking).
- **Bench gaps BN01–03:** the bench measures only the cheapest already-tuned
  core layer; nothing measures `view→diff→flush` or per-widget render — the
  actual hot path. ADR 0005 sanctions adding these.

Recurring small wins (S each, mechanical): `self.block.clone()`→move
(DIFF-7, DP-1, W5-STEP-3); per-frame `format!`/`to_string` for static or
tiny content (calendar W1-01, stepper W5-STEP-2, `status_bar` W5-STATUS-1,
gauge, line_number_gutter, pagination, kitchen-sink KS07); per-keystroke
double `byte_at` walk (CM-4); `String::from_utf8_lossy().into_owned()` where
the `Vec` could move (HOST-5).

Confirmed clean (no action — reference implementations): `ScrollState`
(core), `tree`, `scrollbar`, `modal`, `popover`, `radio`, `skeleton`,
`switch`, `sparkline`, `spinner`, `tooltip`, `checkbox`, `divider`, `drawer`,
W1 widgets, `SystemHostEffects`, the event/input path, `xtask` bench split.

---

## Prioritized plan (leverage order)

### Tier 0 — non-breaking, cascades widely, do first

| # | Fix | Files | Effort | Validates with |
|---|---|---|---|---|
| 0.1 | CT-1/CT-2 buffered backend writer; CT-3 per-channel SGR | `crossterm/backend.rs` | S | new `flush` bench (BN01) + existing backend tests |
| 0.2 | `Buffer::diff` flat-slice zip + `row_slice_mut`/`Rect::rows` | `core/buffer.rs`,`geometry.rs` | S–M | `cargo xtask bench buffer/diff` (`min`, all 4 shapes) |
| 0.3 | RT-01 gate `render` on change + core-io P1-2 `reset`→`fill` | `runtime/run.rs`,`core/buffer.rs`,`core/terminal.rs` | M | new idle-frame bench (BN01) |
| 0.4 | CR-05 `Layout::split_into` + scratch-free `solve` | `core/layout.rs` | M | `cargo xtask bench layout/split/nested` |
| 0.5 | `set_str`/`set_style`/`clear_region`/`resize` → row-slice | `core/buffer.rs` | S | `buffer/set_str`,`buffer/clear_region` bench |

### Tier 1 — non-breaking, per-file, parallelizable

`Buffer::diff` streaming form + `Terminal::flush` reuse (CR-02/core-io P1-1,
M) · `Paragraph` PG-1 early-exit windowing (S, huge) + PG-2 count-only path
(M) · `Table` T1 clone-once + T3/T5 window scans (S–M) · core editing
CM-2/CM-3 cached char-len, CM-4 single `byte_at` (M/S) · `Selection::contains`
frame-scoped span (CM-1, M) · the `block.clone()`→move trio (S) · per-frame
`format!`/scratch-`Vec` removals across calendar/stepper/status_bar/gauge/
gutter/pagination/breadcrumb/avatar (S each) · runtime RT-02 scratch
`VecDeque`, RT-07 inline-1 `effects`, RT-03 single `Instant::now` (M/S) ·
plugin-host PROTO-1, MSG-3 capacity hints, HOST-5 utf8 move (S) ·
acp-client APP-4 id→`ToolCallInfo` map, APP-1/APP-3 cap growth, UI-3 borrow
footer, DRV-1/2 typed `sacp` match (S–M) · kitchen-sink KS01/KS02/KS05
memoize-on-State (M).

### Tier 2 — durable, API-additive (design decisions; sequence after 0/1)

Caller-owned cached layout models: `MarkdownDoc` (MD-1), `ParaLayout`,
`MermaidLayout` (MM-1), `DiffModel` (DIFF-1) — L each, the big content-widget
wins · `Paragraph` "compose-once → (rows, count)" API (kills measure-twice in
Toast/DescriptionList/Table) · `List`/`Table` borrowed/windowed constructor
or `Cow<[_]>` (LIST-1, T6, CP-1) — L · `Stepper`/`Tabs` borrowed-slice
constructors (W5-STEP-1, W5-TABS-1) — M, API-breaking, decide together ·
plugin-host PROTO-3 borrowed/`Cow` `Frame` payload — L · acp-client UI-1/UI-2
per-`Entry` render memo — M/L · `Cell` 16→8 B packing (CR-07) — L,
API-breaking, last.

### Tier 3 — validation infrastructure (do alongside Tier 0)

Batch J: bench scenarios BN01 (`view→diff→flush` idle + 1-widget-changed,
driven through the public loop like `rstui-smoke`), BN02 (per-widget render:
Table/Tree/List/Paragraph/Markdown), BN03 (TextArea/Selection); a
behind-cfg global allocator counter so "allocations per frame" — the metric
this whole review is about — becomes measurable.

---

## Conflict-free implementation batches (Phase 2)

The audit was partitioned by non-overlapping files, so implementation can be
too — each batch owns a disjoint file set; run each through `cargo xtask ci`,
then one locked merge-back per batch (per `docs/merging.md`; never batch to
session end). Re-run `cargo xtask bench` on any batch touching a hot path and
refresh `docs/benchmarking.md` in the same slice.

- **A · `crossterm/backend.rs`** — CT-1/2/3/4. Isolated; do first (biggest
  win, zero cross-stream risk).
- **B · `core/buffer.rs` + `core/geometry.rs`** — CR-01/02/03/04/06/08 +
  core-io P1-2 `reset`. The enabling core change; B unblocks much of G.
- **C · `core/layout.rs`** — CR-05. Independent file.
- **D · `core/text.rs` + `core/style.rs`** — P0-2, P2-1, P3-1.
- **E · `core/{text_edit,text_area,selection}.rs`** — CM-1/2/3/4.
- **F · `runtime/{run,cmd}.rs`** — RT-01/02/03/05/07. RT-01's reset half
  depends on B (`Terminal`/`Buffer`); sequence B→F or let B own the reset.
- **G · `rstui-widgets/*`** — by the W1–W6 audit groups (every widget file is
  independent). Tier-1 widget fixes here; Tier-2 API changes need an ADR note.
- **H · `plugin-host/{protocol,message,host}.rs`** — PROTO-1, MSG-3, HOST-5
  (Tier 1); PROTO-3 (Tier 2).
- **I · `rstui-acp-client/*`** — APP-4, APP-1/3, UI-3, DRV-1/2 (Tier 1);
  UI-1/2 memo (Tier 2).
- **J · `rstui-bench/scenarios.rs`** — BN01–03 + alloc counter. Do early so
  every other batch has a before/after number.

Recommended order: **J** (so wins are measurable) → **A** → **B** → **C/D/E**
(parallel) → **F** → **G** (parallel by widget group) → **H/I** (parallel) →
Tier-2 ADR + design.

---

## Appendix — per-area report index

Raw evidence (gitignored, regenerable): `target/perf-audit/`.

`core-render.md` (CR-01..10) · `core-style-text.md` (P0-1..P3-1) ·
`core-models.md` (CM-1..10) · `core-io.md` (P1-1..P3-3) ·
`widgets-w1.md` (W1-01..04) · `widgets-w2.md` (DIFF/FORM/DESC/CP/DP/FLOW/
EDIT) · `widgets-w3.md` (MD-1..8, LIST-1, MENU-1/2, HELP/GRID/GAUGE/GUT) ·
`widgets-w4.md` (MM-1..4, PG-1/2, PA/SB/SV) · `widgets-w5.md`
(TOAST-1, STATUS-1, STEP-1..3, TABS-1, SPLIT-1) · `widgets-w6.md`
(T1..6, TR1..3, L1) · `runtime.md` (RT-01..08) · `crossterm.md`
(CT-1..8) · `acp-client.md` (UI-1..8, APP-1..6, DRV-1..5, HOST-1/2) ·
`plugin-host.md` (PROTO-1..3, MSG-1..3, HOST-1..5, PROC-1/2, SDK/ACP) ·
`misc.md` (KS01..09, BN01..03).
