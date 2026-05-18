# ADR 0025: Caller-owned line cache — the `LineCache` seam

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** rstui maintainers
- **Amends:** [ADR 0012](0012-widget-composition-and-layout-model.md) (the
  pure-projection model — this names its sanctioned caching seam; it does
  not weaken "no retained widget tree")

## Context

rstui is immediate-mode pure projection (ADR 0012): a widget re-derives its
output from its inputs every frame, owning nothing across frames. For cheap
widgets that is the whole point and it is free.

For a widget whose input is a *language* it is not. `Markdown`,
`Mermaid`/`Structurizr`/`JsonCanvas` run a hand-written parser **and** a
layout engine over the entire source every frame and discard ~94 % of it.
[`docs/perf-review-3.md`](../perf-review-3.md) measured `widget/markdown/render`
at ~1.5 ms — the heaviest widget by ~30×, ~18 % of a 120 fps (8.33 ms)
frame, and a scroll-clamp screen calls it *twice* a frame.

The fix had already been invented **four times, ad hoc**: the acp-client
`Entry.md_cache` (perf-review-1 UI-1/MD-1), `rstui_ai::ConversationCache`
(perf-review-2 R2-AI-1), `rstui_widgets::DiagramCache` (embedded diagrams),
and `Mermaid::from_graph` (flowchart). Four spellings of one idea is the
signal it should be **one named, documented pattern**, not a fifth bespoke
struct (perf-review-3 R3-1).

## Decision

There is one sanctioned shape for "memoise an immediate-mode widget's heavy
per-frame derivation": a **caller-owned, key→`Rc<[Line<'static>]>`
read-through cache**, lived in the app's model exactly like a
`ScrollState`, that the widget *reads through* in `view`.

- One internal generic primitive, `rstui_widgets::line_cache::LineCache<K>`
  (crate-internal): a bounded (`MAX_SLOTS`, clear-on-overflow → total)
  `RefCell<HashMap<K, Rc<[Line]>>>` with a single `resolve(key, compute)`.
- Public wrappers are thin newtypes choosing the **exact, complete** key:
  `DiagramCache` (`(source, width)`) and `MarkdownCache`
  (`(source, width, focused_link, diagrams, theme)` — every input
  `Markdown::lines` reads; a source-only key would render stale link
  highlighting, the real MD-1 wrinkle, here made impossible).
- The seam is **opt-in and additive**: no cache attached ⇒ behaviour is
  exactly as before. Attached, the first frame at a key misses and computes
  **on the unchanged code path**; later frames are `O(1)`.
- It does **not** weaken ADR 0012. The cache is caller-owned *model* state
  (the `ScrollState`/`Input`/`Editor` precedent), never widget interior;
  render output stays a pure function of inputs (a hit and a miss are
  **byte-identical**, gate-enforced by a `cached ≡ uncached` exactness test
  per wrapper). The `RefCell` only elides recomputation; it never changes
  what is rendered. "Caller-side caching is the pure-projection answer to a
  per-frame parse" — the perf dual of caller-side windowing.

## Consequences

- `widget/markdown/cached` ≈ 0.10 ms vs `widget/markdown/render` ≈ 1.5 ms
  (~15×); `widget/markdown/diagrams_cached` ≈ 28 µs vs ~4.4 ms (~150×).
  Cached, the heaviest widget is back in the windowed-widget class and a
  120 fps budget has ~80× headroom for it.
- The cache must be **owned by the model and threaded to every call site**
  that renders the same document (a scroll-clamp screen renders *and*
  measures it) — drift between sites just means extra misses, never a wrong
  render, but defeats the win. Worked reference: the kitchen-sink
  `rich_text` screen (`State.md` / `State.diagrams`, one `doc_md(..)`).
- `rstui_core::Style`/`MarkdownTheme` gained a derived `Hash` (additive,
  std-only, ADR 0001/0003-clean) so the theme can be part of the exact key.
- New heavy widgets (`Structurizr`/`JsonCanvas`/Mermaid-keyword
  `from_parsed`, perf-review-3 R3-3) adopt this seam rather than inventing
  another; `ConversationCache` remains its own `(id, fingerprint, width)`
  variant by design (it memoises a *height*, not lines, for scroll math).
- Not a CI gate: validated by the slow `cargo xtask bench`/`perf` loop
  (ADR 0005) plus the per-wrapper byte-identical exactness tests, which
  *are* gate-enforced.

## Alternatives rejected

- **A retained widget/element tree** — the thing ADR 0012 exists to
  forbid; reintroduces the stale-state bug class for no per-frame win the
  caller-owned cache does not already capture.
- **Widget-interior memoisation** — a widget caching across frames in its
  own state violates ADR 0012 and cannot work (widgets are constructed
  fresh and dropped each frame).
- **A lossy hash key** (e.g. `u64` fingerprint) for `DiagramCache` — a
  collision would render the *wrong* diagram; the exact `(String, …)` key
  has zero collision risk and the per-frame string hash is sub-µs against
  the millisecond it saves. (`ConversationCache` accepts a coarse
  fingerprint only because a miss there yields a slightly stale *integer*,
  never a wrong render.)
