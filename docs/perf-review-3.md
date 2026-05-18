# Performance review 3 — the 120 fps question

- **Date:** 2026-05-19
- **Tooling:** `cargo xtask perf` + `docs/perf-baseline.json` + `rstui-bench`
  + `rstui-devtools` (ADR 0018)
- **Predecessors:** [`docs/perf-review.md`](perf-review.md) (the one-shot
  audit + ~40 landed fixes — two root causes, fixed),
  [`docs/perf-review-2.md`](perf-review-2.md) (repeatable tooling +
  `ConversationCache`)
- **Question asked:** *review every widget and the framework; what do we do
  to make the whole thing run at, ideally, 120 fps?*

## TL;DR

**The framework is already 120 fps-ready with ~100× headroom. 120 fps is
not a framework problem — it is a per-widget *content re-derivation*
problem, and the fix is the caller-owned-cache seam this codebase has
already proven four times.** A full representative app frame
(`runtime/frame/idle`, a real 2-pane app driven through the public loop) is
**~80 µs — about 1 % of an 8.33 ms / 120 fps frame**. Every well-behaved
widget is ≤ 1 %. The *only* things that miss the budget are the handful of
content widgets that **re-parse their whole source every frame** (Markdown
1.48 ms, an uncached Mermaid/Structurizr/Canvas 1–5 ms). They already have
the answer in one place (`DiagramCache`, this session) and three others
(`ConversationCache`, acp-client `Entry.md_cache`, `Mermaid::from_graph`).
The work is to **generalise that seam to every heavy widget and document it
as the pattern**, not to rewrite the engine.

> **Landed on `origin/main`** (measured, gate-green, byte-identical):
> - **R3-1 — DONE.** One internal generic `LineCache<K>` (bounded
>   read-through memo); `DiagramCache` refactored to wrap it (public API +
>   tests byte-identical); the pattern documented in
>   [ADR 0025](adr/0025-caller-owned-line-cache.md) +
>   [`docs/composition.md`](composition.md). One primitive, not four.
> - **R3-2 — DONE.** `rstui_widgets::MarkdownCache` (wraps `LineCache`),
>   keyed by the exact `(source, width, focused_link, diagrams, theme)`
>   tuple (the MD-1 focus/theme wrinkle made impossible);
>   `Markdown::cache(&c)` opt-in builder; the kitchen-sink `rich_text`
>   screen owns one. Enabler: additive `Hash` on `rstui_core::Style` /
>   `MarkdownTheme` (std-only, ADR 0001/0003-clean). Gate: a
>   render-and-`lines()` byte-identical exactness test that also pins every
>   input as part of the key. **`widget/markdown/render` 1.49 ms →
>   `widget/markdown/cached` 0.10 ms (~15×)** — the heaviest widget is back
>   in the windowed-widget class (~1.2 % of a 120 fps frame).
> - **R3-5 — DONE (for R3-1/2).** `widget/markdown/cached` scenario added
>   (the diagrams `_render`/`_cached` template); render vs cached measured
>   apples-to-apples on one shared doc.
> - **R3-3 — DONE (Structurizr + JSON Canvas).** Additive parse-free
>   `Structurizr::from_workspace(&Workspace)` /
>   `JsonCanvas::from_parsed(&Canvas)` (their `parse()` was already public)
>   — the exact `Mermaid::from_graph` shape: `render` lays a caller-held
>   AST out directly and never parses. Byte-identical to `new(src)`
>   (gate-enforced, cell-for-cell × sizes/view-pager); the kitchen-sink
>   `rich_text` Structurizr/JSON-Canvas tabs now parse the `const` once in
>   `State` (the MM-1/2 seam) since the global tick re-renders every frame.
>   `widget/{structurizr,json_canvas}/{render,cached}` benches added.
>   **Mermaid-keyword `from_parsed` — deliberately deferred:** the 22
>   keyword types have no shared AST (22 bespoke per-type parsers); a
>   `from_parsed` each is a large, higher-risk refactor disproportionate to
>   the leverage — the *embedded* case (a keyword diagram in a Markdown
>   doc) is already fully covered by `MarkdownCache`/`DiagramCache`, and a
>   standalone keyword diagram re-parsed every animated frame is not a
>   real app hot path (flowchart, the common one, has `from_graph`). Noted,
>   not a residual defect.
> - **R3-4 — pending** (next slice; the runtime `FRAME_BUDGET`). The
>   conflict-free batch order below stands.

## The budget

| target | frame budget | a `runtime/frame` (~80 µs) is | headroom |
|---|---|---|---|
| 60 fps | 16.667 ms | 0.48 % | ~208× |
| **120 fps** | **8.333 ms** | **~1.0 %** | **~104×** |
| 240 fps | 4.167 ms | ~2.0 % | ~52× |

The framework renders **event-driven, with no fixed-FPS pacing loop**
(`run.rs`: the loop blocks on `poll_event`, renders on input/tick/command,
and RT-01 skips the repaint when an event produced no message). There is no
60 fps cap to lift: "fps" is just how fast one event becomes one frame.
So 120 fps readiness reduces to a single question per screen: **does the
heaviest widget on it turn state into cells in under 8.33 ms?**

## Current numbers (`cargo xtask perf`, this machine)

Every scenario is **within ±10 % of the checked-in baseline** — review 1/2's
fixes have held with zero drift. The `% frame` column is the share of one
**120 fps** (8.333 ms) budget.

| scenario | min | % of a 120 fps frame | read |
|---|---|---|---|
| `buffer/clear_region` | 542 ns | 0.006 % | core stamping |
| `layout/split/nested` | 83 ns | 0.001 % | CR-05, holding |
| `edit/textarea/insert` | 41 ns | ~0 % | CM-3 O(1), holding |
| `buffer/diff/identical` | 11.6 µs | 0.14 % | **idle-frame diff — review-1 win holding** |
| `buffer/diff/full` | 14.4 µs | 0.17 % | full repaint ≈ idle: diff is not the floor |
| `selection/extract` | 26.9 µs | 0.32 % | windowed |
| `widget/tree/render` | 18.6 µs | 0.22 % | clean windowed exemplar |
| `widget/table/render` | 32.1 µs | 0.39 % | windowed |
| `widget/list/render` | 33.3 µs | 0.40 % | windowed |
| `widget/paragraph/render` | 56.2 µs | 0.67 % | post-PG-1 |
| `runtime/frame/idle ≈ changed ≈ mouse_move` | ~80 µs | **~1.0 %** | **the whole 2-pane app frame** |
| `widget/markdown/diagrams_cached` | 28.4 µs | 0.34 % | **the fixed pattern — proof the seam works** |
| **`widget/markdown/render`** | **1.48 ms** | **17.8 %** | re-parses all CommonMark every frame |
| **`widget/markdown/diagrams_render`** | **4.41 ms** | **53 %** | uncached embedded diagrams (cached → 0.34 %) |

There is a **two-order-of-magnitude cliff**: everything is ≤ 1 % of a
120 fps frame *except* the content widgets that re-derive their whole input
every frame. `diagrams_render` (4.41 ms, 53 %) vs `diagrams_cached`
(28 µs, 0.34 %) — **a ~150× gap that is entirely the cache** — is the whole
report in one row.

## Framework-loop 120 fps analysis

The engine itself needs **no structural change** for 120 fps. Three loop
constants touch the boundary; one is a latent papercut:

- **`COALESCE_TIME_BUDGET = 8 ms`** (`run.rs:172`) — the wall-clock cap on
  folding an input flood before a forced repaint. It pins the worst-case
  cadence under a continuous flood to ~125 fps, which is *coincidentally*
  ≈ a 120 fps frame. It is correct today but is a **magic `8`**: it should
  be **derived from a single target frame budget**, so the flood floor
  tracks the fps goal instead of silently being "120-ish".
- **`COMMAND_POLL_INTERVAL = 16 ms`** (`run.rs:148`, `run_threaded` only) —
  an off-loop `Cmd::perform`/`tick` result can repaint up to 16 ms late
  (~2 × a 120 fps frame). Fine at 60 fps; for a 120 fps app it is the one
  place background work feels a frame behind. Should also derive from the
  frame budget.
- **RT-01 produced-gating + `buffer/diff` (~12 µs)** — already optimal
  (review 1). An idle screen does **not** repaint; a no-op event does not
  diff. Nothing to do.

**Recommendation R3-4:** introduce one `FRAME_BUDGET` (from a target-fps
constant, default 120) and express `COALESCE_TIME_BUDGET` and
`COMMAND_POLL_INTERVAL` as fractions of it. Pure internal constants, no API
change, removes two magic numbers and makes the fps target explicit and
tunable.

## The single root cause (restated for 120 fps)

Review 1 had two root causes; both are fixed and holding (the table proves
it). Only one *class* of cost remains, and it is inherent to the model, not
a defect: **immediate-mode pure projection (ADR 0012) re-derives every
widget's output from its inputs every frame.** For cheap widgets that is the
whole point and it is free (≤ 1 %). For a widget whose input is a *language*
— Markdown, the Mermaid family, the Structurizr DSL, JSON Canvas — "derive
from inputs" means **run a hand-written parser + a layout engine over the
entire source, every frame, and discard ~94 % of it** (off-screen rows).
At 60 fps a small such widget hides under the budget; at 120 fps the budget
halves and a single large one (a long help doc, an architecture diagram)
**is** the frame.

The ADR-0012-compliant answer is *not* a retained tree. It is a
**caller-owned cache**: the immutable parse/layout lives in the app's model
(the `ScrollState`/`Input`/`Editor` seam), the widget reads through it, a
hit and a miss are byte-identical (gate-enforced). This codebase has now
built that **four times, ad hoc**:

1. acp-client `Entry.md_cache` (review 1, UI-1/MD-1)
2. `rstui_ai::ConversationCache` (review 2, R2-AI-1)
3. `rstui_widgets::DiagramCache` (this session — embedded diagrams 4.41 ms → 28 µs)
4. `Mermaid::from_graph(&MermaidGraph)` (review 1, MM-1 — flowchart only)

Four implementations of one idea is the signal that it should be **one
documented framework pattern**, not a fifth bespoke struct.

## Cache-coverage matrix (code-verified)

Class **A** = cheap projection, 120 fps-safe. **B** = heavy, *has* a
caller-cache/parse-free seam (safe iff the caller uses it). **C** = heavy,
**no seam** — a 120 fps risk for large input.

| widget | per-frame cost (file) | seam | class |
|---|---|---|---|
| `Markdown` **prose** | `blocks_into`+`collect_defs`+`layout_blocks` parse all source every `lines()` (`markdown.rs:618`); 1.48 ms; called **2×/frame** by scroll-clamp screens | `new`/`lines` only — **no `parse()`/`from_parsed`**; `DiagramCache` covers *only* embedded diagrams, not the prose | **C** |
| `Mermaid` keyword types (22: sequence/class/state/gantt/…) | each `*::render(src,…)` re-parses the DSL every frame (`mermaid/mod.rs:875+`); 1–5 ms | none (`from_graph` is **flowchart-only**, `mermaid/mod.rs:678-681`) | **C** |
| `Structurizr` | `render` calls `parse_workspace(self.source)` every frame (`structurizr.rs:480`) | `parse()→Workspace` is public (`:458`) but **no `from_workspace`** to feed it back | **C** |
| `JsonCanvas` | `render` calls `JsonCanvas::parse(self.source)` every frame (`json_canvas.rs:669`) | `parse()→Canvas` public (`:643`) but **no `from_parsed`** | **C** |
| `stream_markdown` | `remend()` + `parse_into_blocks()` over the whole buffer every frame/token; per-block lines *are* cached | per-block `StreamCache`, but the repair+segmentation is not memoised | **B−** |
| `Markdown` embedded diagrams | parse+layout+rasterise each fence | **`DiagramCache`** `(source,width)` memo (`.diagram_cache(&c)`) | **B** |
| acp `Conversation`/`Message` | full Markdown parse per message per frame for height/scroll | **`ConversationCache`** `(id,fp,width)` memo | **B** |
| `Mermaid` **flowchart** | `parse_graph`+`lay_out` ~800 LOC | **`from_graph(&MermaidGraph)`** (parse held in model) | **B** |
| `Diff` | DIFF-1 windows layout to the viewport; still re-parses the patch text | windowed (caller-cap), bounded by viewport not patch size | **B** |
| `List`/`Table`/`Tree`/`DataTable` | windowed: `.skip(offset).take(height)`, never all-N | windowing is intrinsic; `DataTable.project` is reducer-owned | **A** |
| Charts (`line/bar/scatter/candlestick/pie/sparkline/heatmap/treemap/…`) | one O(points) walk of caller-pre-reduced data; no parse | input *is* the derived model (caller's reducer owns it) | **A** |
| `Paragraph`, `Block`, leaf/form/container widgets, `rstui-jsonui` `UiNode` | O(area) stamping / deterministic wrap; jsonui doc parsed at message boundary not render | pure projection | **A** |

Class A is confirmed clean — reference implementations, **no action**. The
report is entirely about converting **C → B** with one uniform seam.

## Prioritised plan (leverage order)

Leverage = (cost × how-often-rendered-large) ÷ effort. Each item is
**additive, ADR-0012-compliant, byte-identical (gate-enforced like
`DiagramCache`/`ConversationCache`)**, and **opt-in** (no cache attached ⇒
exactly today's behaviour).

### R3-1 — Generalise the caller-owned-cache seam *(the headline)*

Promote the four ad-hoc caches to **one documented pattern + one reusable
primitive**, so every Class-C widget gets an identical 120 fps path:

- A small generic `(source,width)`-keyed read-through memo in
  `rstui-widgets` — `DiagramCache` *is* the template; lift its shape into a
  reusable form (the `ParseCache<P>` / per-widget-cache pattern) usable by
  Markdown/Mermaid/Structurizr/JsonCanvas.
- Document it in [`docs/composition.md`](composition.md) and an **ADR** as
  *the* virtualization/perf answer ("caller-side caching is the
  pure-projection answer to a per-frame parse"), with the byte-identical
  exactness-test discipline as the contract.
- Effort: **M** (the primitive is ~`DiagramCache` again, generalised). This
  is the durable win; R3-2/3 are its instances.

### R3-2 — `Markdown` prose cache *(highest single-widget leverage)*

Markdown is the heaviest widget (1.48 ms, 17.8 % of a 120 fps frame) **and**
the most-rendered content widget (help, docs, chat, rich-text). Add
`Markdown::parse() -> ParsedMarkdown` + a `from_parsed`/`MarkdownCache`
seam. **Wrinkle (from review-1 MD-1, still true):** `blocks_into` depends on
`focused_link` *and* `theme`, so the cache key is
`(source, width, focused_link, theme)`, not source alone — a naive
source-only cache renders stale link highlighting (a correctness bug static
snapshots miss). This is exactly why MD-1 was ADR-gated; R3-1's primitive
should make the key explicit. Effort: **M–L**.

### R3-3 — `from_parsed` for Mermaid-keyword / Structurizr / JsonCanvas

`Structurizr::parse()→Workspace` and `JsonCanvas::parse()→Canvas` are
**already public**; only the parse-free render constructor is missing. Add
`Structurizr::from_workspace(&Workspace)`, `JsonCanvas::from_parsed(&Canvas)`,
and extend the Mermaid keyword path with a cached AST (or document
`from_graph` and add the per-type analog). Purely additive — mirrors
`Mermaid::from_graph`. Effort: **S–M each**, parallelizable.

### R3-4 — Runtime-loop 120 fps tuning

`FRAME_BUDGET` from a target-fps constant; `COALESCE_TIME_BUDGET` and
`COMMAND_POLL_INTERVAL` derived from it; document the 120 fps stance in
`docs/runtime.md`. No API change. Effort: **S**.

### R3-5 — Close the measurement gap *(do first, like Batch J)*

Add first-party bench scenarios for the Class-C widgets and their cached
twins (`widget/mermaid/{render,cached}`, `…/structurizr/…`,
`…/json_canvas/…`) — the `widget/markdown/diagrams_{render,cached}` pair
this session added is the template, and rstui-widgets is first-party so this
is ADR-0005-allowed. Then `cargo xtask perf --save` to re-baseline (the two
new diagram scenarios are currently unbaselined). Effort: **S**. Do this
first so every R3-2/3 slice lands with a before/after number.

### Accepted as-is / no action

Class A (charts, list/table/tree/datatable, paragraph, leaves, jsonui) —
reference implementations. `Diff` (B, viewport-windowed) and
`stream_markdown` (B−, per-block cached; a buffer-hash short-circuit of
`remend()` is a small optional follow-up, not a 120 fps blocker).

## Feasibility verdict

**120 fps is achievable framework-wide.** The engine has ~100× headroom and
needs no structural change (R3-4 is polish). The *only* determinant is
whether an app feeds heavy content widgets through a cache seam — and the
seam is proven (`diagrams_cached`: 53 % → 0.34 % of a 120 fps frame). After
R3-1..R3-3 every widget has a uniform, documented 120 fps path; without
them, one large uncached Markdown/Mermaid on screen is, by itself, ~2–6
120 fps frames. 240 fps is then also reachable for any cached/Class-A
screen (still ~50× headroom).

## Conflict-free implementation batches

Partitioned by disjoint files (per [`docs/merging.md`](merging.md); serial
locked land, never batch to session end; re-run `cargo xtask bench` +
refresh the baseline in any hot-path slice):

- **J · `rstui-bench/scenarios.rs`** — R3-5 scenarios + re-baseline. First.
- **A · `rstui-widgets` cache primitive** — R3-1 (generalise `DiagramCache`)
  + the composition.md/ADR note.
- **B · `markdown.rs`** — R3-2 (depends on A's primitive; the
  `(source,width,focused_link,theme)` key).
- **C · `mermaid/`, `structurizr.rs`, `json_canvas.rs`** — R3-3, three
  independent files, parallel.
- **D · `rstui-runtime/run.rs`** — R3-4, isolated.

Recommended order: **J → A → (B ∥ C ∥ D)**.

## Constraints (unchanged from review 1, ADR-verified)

- **Pure projection, no retained tree** (ADR 0012): caching is
  *caller-owned* model state, never widget interior — the `ScrollState`
  seam. `DiagramCache` (a caller-owned `RefCell` read-through memo, render
  output still a pure function of inputs, hit≡miss gate-enforced) is the
  sanctioned shape.
- **`rstui-core` is dependency-free, `unsafe` forbidden** (ADR 0001/0003):
  std only.
- **Validation is the slow loop** (ADR 0005): `cargo xtask perf`/`bench`
  (`min` is the signal), not a CI gate; the byte-identical render/measure
  exactness tests *are* the gate.

## Appendix — method

20 release `rstui-bench` scenarios (1000 iters/100 warmup) → `cargo xtask
perf` diff vs `docs/perf-baseline.json` (all `ok`); a read-only fan-out
audit of every `rstui-widgets`/`rstui-ai`/`rstui-jsonui` content widget for
the review-1 root-cause-B pattern; and `file:line` verification of each
widget's parse/seam (Markdown has no `parse()`/`from_*`; `Mermaid::from_graph`
is flowchart-only per its own doc-comment; `Structurizr`/`JsonCanvas` expose
`parse()` but no `from_parsed`) — the derive-from-code discipline of
review 1, not assertion.
