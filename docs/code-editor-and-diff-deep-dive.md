# Code editor & diff: colour, scrolling, selection, symbols — deep dive

**Question this answers:** for the `Editor` and `Diff` widgets specifically —
how does [`modem-dev/hunk`](https://github.com/modem-dev/hunk) colour code and
what transfers; what *exactly* is the scrolling bug and how is it fixed;
what must be added to (a) select text then replace it and (b) show a
**symbol/outline side panel** for both the editor *and* the files in a diff;
and **what other gaps** in these two widgets must we resolve.

This is a deep-dive and design document. Its sibling
[code-review-and-editing.md](code-review-and-editing.md) is the broad vim+hunk
capability audit; **this doc deepens four of its threads and adds two it
deliberately scoped out** (real syntax colour; a symbol outline). The engine
decision is recorded in
[ADR 0022](adr/0022-syntax-colour-and-symbol-engine.md); the primitives are
landed (`scroll_into_view`, span edits, `undo`, `doc_selection`, `search`,
`syntax`, `outline`, `changeset`) and the consumer wiring is sliced in Part 7.

## TL;DR

- **hunk does not colour code itself.** It is TypeScript on Shiki (TextMate
  grammars + Oniguruma WASM); it has **no symbol panel**. The transferable
  idea is the *layering*: highlight the code first, then overlay add/remove
  background, then overlay an intra-line word-diff emphasis background while
  **keeping the syntax foreground**. `Diff` already does exactly this 3-layer
  cascade (`diff.rs` `content_spans`). The gap is not the model — it is the
  **quality of layer 1** and that **`Editor` has no layer 1 at all**.
- **The scrolling bug is real, has three distinct faces, one root.** The
  `Editor` widget deliberately defers `scroll_into_view`; `rstui-git-review`
  therefore (1) recomputes editor scroll every frame as
  `crow.saturating_sub(4)` — unclamped, viewport-blind, not stored; (2) draws
  the line-number gutter as a fixed `1..=row_count` that **never scrolls with
  the text**; and (3) clamps `diff_scroll` to the *total line count* instead
  of *total minus viewport*. Root: **nothing feeds viewport geometry back
  into the model**, so the reducer that ADR 0004 says owns scroll cannot
  clamp or follow the caret. Fixed by the pure, total
  `TextArea::scroll_into_view` seam (landed).
- **Select-then-replace** is now possible below the widget:
  `TextArea::{span_text,delete_span,replace_span}` + the caller-owned
  `DocSelection` (landed); the `Editor` projection + git-review wiring is
  Part 7.
- **A symbol panel is greenfield** (neither hunk nor rstui had one): the
  caller-owned `Outline` model + dependency-free heuristic scanner is landed;
  the diff side joins it to the `Changeset` model (also landed).
- **The other gaps** (Part 6): no undo while git-review *writes files on
  Ctrl-S* (data-loss — `undo` landed, wiring Part 7), no horizontal scroll in
  `Diff`, editor clips instead of wrapping, a literal tab renders as one
  cell, `Diff::lines()` re-parses per keypress, language-blind syntax, no
  search. Severity-ranked, with which are tracked vs newly surfaced.

## The rule every new piece obeys

From [ADR 0004](adr/0004-focus-routing-architecture.md),
[ADR 0012](adr/0012-widget-composition-and-layout-model.md) and
[composition.md](composition.md):

> The **model owns the state.** `view` is a **pure projection**; a widget is
> handed only a `Buffer` at render time, never mutates, never panics, holds no
> retained tree. `update` is the only place state changes. Hit-testing is the
> reducer's job against the `Rect`s it laid out.

So syntax colour, the selection span, the symbol list, and the scroll offset
are all **caller-owned data the widget reads per cell** — exactly as `Editor`
already reads caller-owned `extmarks` and `Selection::contains` proves the
per-cell-flag pattern. The new `Editor::cell_to_doc` accessor (landed) is the
pure inverse for hit-testing. A real syntax-tree highlighter and a real regex
engine remain **optional, feature-gated** tiers, never the default
([ADR 0002](adr/0002-widget-crate-boundary.md), [ADR 0022](adr/0022-syntax-colour-and-symbol-engine.md)).

---

## Part 1 — How hunk colours code, and what transfers

hunk is **TypeScript/Bun on OpenTUI**, MIT licensed. It performs **no
highlighting of its own** — it imports `@pierre/diffs` (Apache-2.0), built on
**Shiki** (`shiki ^3.0.0`): TextMate JSON grammars tokenised by the
**Oniguruma regex engine compiled to WASM**, themed by VS Code-style JSON
themes. Language is chosen by **filename extension**. Highlighting is
**whole-file, not incremental**, made bearable by aggressive caching (a
per-`appearance:language` shared highlighter, a `WeakMap` per HAST node
memoising flatten-to-spans, context-line node aliasing, a 150-entry LRU,
microtask serialisation, render-time horizontal slicing + vertical
windowing + `React.memo`). It has **no symbol/outline panel** — the sidebar is
a flat file list with `+/-` badges; zero tree-sitter/LSP/ctags anywhere.

This is the *opposite* technology choice from a dependency-free Rust TUI, so
the value is the *architecture lessons*, not the code:

1. **The layering order is the keeper, and we already have it.** hunk
   produces syntax-highlighted spans *first*, then overlays add/remove
   background from the line `kind`, then an intra-line word-diff emphasis
   background that **keeps the syntax foreground**. `Diff::content_spans`
   already does precisely this — `row_style → syntax overlay (under) →
   word-diff mark (on top)`. The model is *validated by hunk*; the gap is
   only the **quality of layer 1**.
2. **Word-diff contrast guarantee.** hunk's `strengthenWordDiffBg` blends the
   sign colour into the line background until a minimum perceptual distance —
   a concrete, theme-robust improvement to copy into `DiffTheme`.
3. **Caching is caller-owned memo, our existing idiom.** The reducer owns a
   `Vec<LineHighlight>` keyed by a content fingerprint, rebuilt only on edit
   (the `ConversationCache`/`UI-1`/`MD-1` precedent); the widget just reads
   it. The new `syntax::LexState` carries multi-line string/comment state
   line-to-line so the memo is correct.
4. **No prior art for symbols** — Part 5 is a clean independent design.

---

## Part 2 — The scrolling defect: root cause and fix

`Editor`'s module doc states the deferred contract: a cursor scrolled out of
the window draws **no caret**; keeping it in view is the caller's job. The
render path enforces it literally (caret drawn only if `cur_row >= row_off &&
cur_col >= col_off` and inside the inner rect). Correct and ADR-0004-aligned;
the defect is entirely in the consumer because the deferred seam was never
built.

**The three faces in `rstui-git-review`:** (a) editor scroll computed every
frame as `crow.saturating_sub(4)` — unclamped, viewport-blind, not stored, so
the "only scroll when the caret leaves the view" UX is impossible and long
lines clip the caret off the right edge; (b) the line-number gutter is a
fixed `1..=row_count` that never scrolls with the text, so every number is
wrong once scrolled — a distinct second bug; (c) `diff_scroll` clamps to the
*total row count* (`diff.lines().count()`, an O(parse) call on every motion
key) instead of *total − viewport*, so content scrolls into a blank pane.

**Root cause:** ADR 0004 makes scroll reducer-owned model state, but nothing
feeds the laid-out viewport height back into the model, so the reducer cannot
clamp to `content − viewport` or follow the caret.

**The fix (landed core, wiring in Part 7):**
`TextArea::scroll_into_view(scroll, viewport, margin) -> (row_off, col_off)`
— pure, total, snapshot-tested, gate-enforced by proptest **Invariant 6**
(for every non-zero viewport, any prior offset, any margin, the caret is
*always* inside the returned window and the offsets never run past the end).
The app then owns `editor_scroll` and a `detail_viewport` it learns from
`Event::Resize` (the missing model←geometry feedback — a one-line ADR 0004
clarification: the reducer learns laid-out extents via a resize message; that
is model state, not a `view` mutation), drives the gutter from the same
offset, and clamps `diff_scroll` to `total.saturating_sub(viewport_h)` using
a cached row count. This is the sibling's `E3a`, made concrete plus the
geometry-feedback requirement it missed.

---

## Part 3 — Real syntax colour, for the editor *and* the diff

`Diff` shipped a dependency-free lexical tinter (`diff.rs` `syntax_overlay`):
language-blind (one global keyword set), comments/strings never span lines,
and `Editor` had none. [ADR 0022](adr/0022-syntax-colour-and-symbol-engine.md)
decides **two tiers**:

- **Tier 0 (landed): `rstui_widgets::syntax`** — the lexer extracted into a
  shared module used by both `Diff` and `Editor`, made language-aware
  (`Language::from_path`), with a carried end-of-line `LexState` so multi-line
  strings/comments colour correctly. `Language::Unknown` is **byte-identical**
  to the old scanner (gate-enforced), so existing diff snapshots are
  unaffected. Colours come from the caller's theme (`SyntaxStyles`), so
  `rstui-theme` themes code for free.
- **Tier 1 (Part 7 CE-6, optional): feature-gated tree-sitter** — one
  incremental parse → both the highlight overlay *and* the symbol outline.
  Default builds never compile it; the floor is always present. TextMate/
  Oniguruma was rejected (highlight-only — see ADR 0022 Evidence).

The `Editor` change is a borrowed overlay exactly like `extmarks`: a per-cell
style set the reducer rebuilds on edit and the widget only reads, cascading
**base → focus → syntax → extmark → selection → caret** (syntax beneath
extmark/selection, mirroring the diff cascade).

---

## Part 4 — Select text, then replace it

`TextArea` was a point cursor with no selection and no span edits; the model
`Selection` is render-space (cells), not logical `(row,col)`. **Landed:**
`TextArea::{span_text, delete_span, replace_span}` (total, gate-enforced by
the extended 22-op totality proptest) and the caller-owned `DocSelection`
(`Char`/`Line`/`Block`, the logical dual of `Selection`). "Replace the
selection" is `replace_span(sel.range(), typed)`. Part 7 wires the `Editor`
projection (a `selection` borrow + per-cell `contains`, the proven
`Selection`/`extmark` pattern) and git-review driving: Shift+motion extends
the anchor; mouse uses the landed `Editor::cell_to_doc` with the
`on_press`/`on_pointer_drag`/`on_release` pointer-gesture seam from
composition.md.

---

## Part 5 — A symbol/outline side panel (editor *and* diff)

Greenfield — no prior art. **Landed:** `rstui_widgets::outline` —
`Symbol { name, kind, line, end_line, depth }`, `Outline(Vec<Symbol>)`, a
dependency-free per-`Language` heuristic scanner (`Outline::scan`) and
`Outline::at_line` (deepest enclosing symbol — the "current symbol"). It is a
*model + scanner*, not a new widget: the app projects it through the existing
`Tree`/`List`, and navigation is reducer arithmetic over the ordered vector
(the sibling's `C.4` "the index is the gap, not the state" insight).

**Diff side (Part 7 CE-7):** the landed `rstui_widgets::changeset`
(`Changeset → DiffFile → HunkRef`, multi-file, ordered hunk index) supplies
which file's content to outline (new-side, via the git `Cmd` seam) and a
hunk→symbol join (`Outline::at_line(hunk.new_start)`) → a symbol list
annotated with which symbols changed and a "jump to next changed symbol"
navigation. No new widget — model + `Tree`/`List`.

---

## Part 6 — Other gaps we must resolve

Severity: **S1** correctness/data-loss, **S2** materially broken UX, **S3**
quality. "Tracked?" = already in
[code-review-and-editing.md](code-review-and-editing.md).

| # | Gap | Sev | Tracked? | Status / resolution |
|---|---|---|---|---|
| A | Scroll computed/unclamped/viewport-blind; gutter not scrolled; diff clamp wrong | **S2** | Partly (`E3a`, not the gutter/geometry root) | Core `scroll_into_view` **landed**; git-review wiring Part 7 |
| B | `Diff` no horizontal scroll — long lines hard-clipped, no `←/→` | **S2** | No | Part 7: `Diff::col` + style-preserving horizontal slice; `Editor` col-scroll via `scroll_into_view` |
| C | `Editor` clips, never wraps; `content_height` measures *as if* wrapped (mismatch) | **S2** | No | Part 7: `wrap: bool` projection mode; horizontal-scroll affordance (B) until then |
| D | A literal **tab renders as one cell** — source indentation collapses | **S2** | No | Part 7: caller-set `tab_width` expansion seam in the shared render path |
| E | **No undo** while git-review **writes the file on Ctrl-S** — data-loss | **S1** | Sibling `B.6` flags undo generally, not the live write path | `undo` **landed**; Part 7 gates Ctrl-S behind it |
| F | No selection / span edits → no select-then-replace | **S2** | `E1b/E1e/E3a` | `DocSelection` + span edits + `cell_to_doc` **landed**; Part 7 projection/wiring |
| G | Syntax language-blind, never spans lines; `Editor` has none | **S2** | Non-goal in sibling | `syntax` **landed** (ADR 0022); Part 7 swaps `Diff`, adds `Editor` overlay |
| H | No symbol/outline for editor or diff | **S2** | No | `outline` + `changeset` **landed**; Part 7 panels |
| I | `Diff` single-patch, read-only: no changeset/stream/index | **S2** | `C.1–C.4` | `changeset` **landed**; stream widget is sibling `R1b` |
| J | No search/highlight in `Editor` or `Diff` | **S3** | `B.7` | `search` **landed**; Part 7 UI |
| K | `Diff::scroll: u16` caps at 65 535; `Diff::lines()` re-parses every keypress | **S3** | No | Part 7: widen to `usize`; cheap cached `row_count`; clamp off `lines()` |
| L | `Editor` no mouse seam | **S3** | Implied by `E3a` | `Editor::cell_to_doc` **landed**; Part 7 wiring |
| M | `content_height` silently saturates at `u16::MAX` | **S3** | No | Part 7: measure in `usize` / document the saturation |

The S1/high-impact, sibling-missed items escalated: **E** (live data-loss),
**A**'s gutter-desync + geometry-feedback root, **G** bringing layer 1 to the
`Editor` at all, and **B/C/D** which together make the editor unusable for
real source files.

---

## Part 7 — Consolidated plan (composes with the sibling's waves)

New files were conflict-free and are **landed**; the remaining slices are
additive method-sets on `diff.rs`/`editor.rs` (serialised, hot files) and the
`rstui-git-review` consumer. Wave IDs prefixed `CE`, slotting beside the
sibling's `E*`/`R*`.

| Slice | Scope | Gap | Pri | Status |
|---|---|---|---|---|
| CE-0 | `scroll_into_view`, span edits, `undo`, `doc_selection`, `search`, `syntax`, `outline`, `changeset`, `cell_to_doc` | A/E/F/G/H/I/J/L cores | **P0** | **Landed** |
| CE-1 | `Diff`: delegate to `syntax` (+`Language`), `col` + horizontal slice, `usize` scroll, cached `row_count`, `tab_width` | A/B/G/K/D | **P0** | Part 7 |
| CE-2 | `Editor`: `syntax` overlay borrow, `DocSelection` projection + caret shape, `tab_width`, `wrap` | C/D/F/G | **P0** | Part 7 |
| CE-3 | `rstui-git-review`: `editor_scroll`+`detail_viewport` (resize), scrolled gutter, `scroll_into_view`, diff clamp/col, undo keys + undo-before-save, selection (mouse+shift+replace), `Language`, symbol panel (editor+diff), search UI | A/B/E/F/G/H/J end-to-end | **P0** | Part 7 |
| CE-4 | tree-sitter feature (ADR 0022 Tier 1) → highlight + outline | G/H accuracy | **P2** | Part 7 |
| CE-5 | `Editor::wrap`, `usize` measure (`content_height`), `outline`/`syntax` `Language` dedup | C/M + tidy | **P2** | Part 7 |

**Critical path:** CE-1 → CE-2 → CE-3 (CE-3's undo wiring gates further
git-review editing because of the live data-loss path).

---

## Part 8 — Explicit non-goals

- **LSP / language servers / semantic rename / cross-file go-to-definition** —
  the symbol panel is *structure of the open/changed file*, not a language
  service.
- **A real regex engine for search** — literal + a documented small subset is
  the floor; a regex crate is an optional feature-gated tier
  ([ADR 0002](adr/0002-widget-crate-boundary.md)).
- **tree-sitter on the default path** — Tier 1 is *always* feature-gated; Tier
  0 is always present ([ADR 0022](adr/0022-syntax-colour-and-symbol-engine.md)).
- **Being a VCS / reading git in core** — file content for the diff outline is
  a `Cmd`/app seam.
- **Soft-wrap as the default** — clip stays the default render; wrap is an
  opt-in projection mode (its own correctness surface).

## See also

- [code-review-and-editing.md](code-review-and-editing.md) — the broad
  vim+hunk audit this doc deepens (Parts 2/4) and revises (Parts 3/5); its
  `E*`/`R*` waves the `CE-*` slices compose with.
- [ADR 0022](adr/0022-syntax-colour-and-symbol-engine.md) — the engine
  decision (tree-sitter optional tier vs rejected TextMate/Oniguruma; the
  dependency-free floor).
- [ADR 0002](adr/0002-widget-crate-boundary.md) ·
  [ADR 0004](adr/0004-focus-routing-architecture.md) ·
  [ADR 0012](adr/0012-widget-composition-and-layout-model.md) ·
  [composition.md](composition.md) — the pure-projection + pointer-gesture
  seams Parts 2/4/5 reuse.
- [git-review.md](git-review.md) — `rstui-git-review`, the worked consumer
  every Part 2/4/5 fix ships its proof in.
