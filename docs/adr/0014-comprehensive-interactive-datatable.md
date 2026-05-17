# ADR 0014: Comprehensive interactive DataTable

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

`Table` (ADR 0012 §P2) is deliberately a minimal, total pure projection of
caller-placed `Row`s with single-row selection and *nothing else*. A
recurring real-application need is the opposite end of that spectrum: a
**spreadsheet-class grid** that sorts, filters, groups (with collapsible
group headers), scrolls a very large data set quickly, is driven by the
mouse, and — newly required — supports **optional per-column in-cell
editing** where the consumer marks which columns are editable and the
application consumes the edit/sort/group/filter changes.

The forces already locked in, which this decision must fit rather than
relitigate:

- **Immediate-mode, pure `view(&self)`** — no retained tree; a widget is
  handed only a `Buffer` at render time and may not mutate (ADR 0012).
- **State is caller-owned; the reducer is the sole mutation point** — the
  `List`/`Tree`/`Input`/`ScrollState`/`Selection` discipline.
- **`rstui-core` is dependency-free**; reusable interaction primitives that
  are *generic* live there (`ScrollState`, `Selection`, `TextEdit`).
- Totality (panic-free for any input) and the `cargo xtask ci` gates
  (fmt, lint-names, clippy `-D`, rustdoc `-D`, test) apply to every slice.

The expensive-to-reverse question is *how* a stateful-feeling grid —
"click a header to sort", "type into a cell" — is expressed without a
retained tree, render-time mutation, or callbacks, and **where the
sort/filter/group data pipeline runs**.

## Decision drivers

1. **Immediate-mode honesty** — no callback, no render-time mutation, no
   second rendering subsystem.
2. **Fast scroll at scale** — scrolling a huge table must be O(visible
   window), not O(rows), every frame.
3. **Single source of truth** — one model owns data, spec, scroll,
   selection, and the in-progress edit; `view` only reads.
4. **Reuse over reinvention** — compose the existing `ScrollState` /
   `TextEdit` / `Input` rather than grow parallel implementations.
5. **Consumer-controlled editing** — editability is a property of the
   caller's column description; the widget never decides to edit.
6. **Totality & discoverability** — one documented pattern, panic-free for
   any input, copyable by humans and agents.

## Options considered

### A. Grow `Table` with sort/filter/group/edit flags

Rejected. It would make every `Table` caller pay for an edit lifecycle and
a data pipeline, and the only way `Table` could sort/edit *itself* at
render time is render-time mutation — exactly what ADR 0012 forbids for the
widget the whole framework depends on being inert.

### B. A `StatefulWidget` that owns the data and mutates on events

Rejected. This is the ratatui pattern ADR 0012 already recorded as
incompatible: it requires a retained, event-receiving widget and breaks the
pure `view(&self)` contract.

### C. A separate pure-projection `DataTable` + a reducer-run pipeline + pure accessors — **chosen**

A new widget, sibling to `Table` exactly as `MaskedInput` is to `Input`.
The data pipeline (**filter → sort → group → collapse → flatten**) is a
pure, total free function `project()` the **reducer** runs once per
data/spec change — the *same* shape `Tree` uses, where the reducer owns the
tree and rebuilds a flattened `Vec` the widget merely reads. Scrolling is a
composed `rstui_core::ScrollState`; in-cell editing is a borrowed
caller-owned `rstui_core::TextEdit` rendered by **reusing `Input`** (one
caret/scroll implementation). Mouse and change events are surfaced as
**pure geometry accessors** (`hit`, `cell_rect`) — the recorded ADR 0012
`SplitPane::divider_rect` / `ScrollView::viewport` model — never callbacks.

## Decision

1. **`rstui_widgets::data_table` is the comprehensive grid; `Table` stays
   minimal.** `DataTable` is a pure projection of caller-owned
   `[DataColumn]` / `[DataRow]` / a flattened `[VisualRow]` /
   `DataTableState` / an optional editing `&TextEdit`.

2. **The data pipeline is the reducer's job, via the pure `project()`
   engine.** `project(columns, rows, state) -> Vec<VisualRow>` applies
   filter → stable lexicographic sort → stable grouping → collapse, and
   returns *indices into the caller's stable source `Vec`*. A source index
   is a stable identity that survives a re-sort — which is what keeps an
   in-progress edit pinned to the correct cell. Render is O(visible
   window): scrolling is independent of row count.

3. **`DataTableState` is the caller-owned interaction state.** It is
   datatable-specific (it carries a collapsed-group set and a sort/filter/
   group spec), so it lives in `rstui-widgets`, not dependency-free
   `rstui-core` — but it *composes* the core `ScrollState` for vertical
   scroll rather than reinventing it (the `ScrollView` precedent). Every
   method is total; a fixed-seed fuzz proves it.

4. **Editing is the `Input` model, consumer-gated.** A column opts in with
   `DataColumn::editable`. When `DataTableState::editing` points at a cell
   in an editable column *and* the caller supplies the `&TextEdit`, that
   one cell is rendered by **reusing `Input`** (a focused field with a
   caret). The reducer owns the keystrokes; `commit_edit()` returns the
   `(source, column)` just edited so the reducer knows exactly what to
   write back into its own data. There are **no callbacks** — a callback is
   render-time mutation by another name.

5. **Change/mouse events are pure accessors.** `DataTable::hit(area, pos)
   -> Option<DataTableHit>` resolves a position to a `Header`/`Group`/
   `Cell`; `cell_rect` returns a visible cell's rect. The reducer maps a
   `MouseEvent`/key through these in `update` and mutates its own data +
   state. This *is* the event hook, and it is the only one consistent with
   immediate-mode (ADR 0012 §1).

6. **Scope is bounded and recorded, not smuggled.** This slice is stable
   single-column lexicographic sort, one case-insensitive substring filter
   across all cells, single-level grouping, full-width selection.
   Multi-key/typed sort, per-column/regex filter, multi-level grouping, 2-D
   cell selection, drag column resize/reorder, and a reserved selection
   gutter are recorded clean future additives over this exact shape (the
   `Tree` "defer cleanly" precedent), not gaps.

## Follow-up (delivered): any form field per cell

The original slice's editing was a single borrowed `TextEdit`. A
follow-up generalises it **additively, non-breaking** (default
`CellField::Text` is byte-for-byte the prior behaviour; `.edit(&TextEdit)`
unchanged), exactly as Decision §3 anticipated — a bounded pure extension,
no model change:

- A per-column **`CellField`** (`Text` / `Checkbox` / `Switch` / `Select`)
  the widget renders by **reusing** the matching widget
  (`Input`/`Checkbox`/`Switch`/`Select`) — no second implementation. The
  cell `Line` stays the single value of record, so sort/filter are
  untouched and the reducer writes edits back the same way (`cell_truthy`
  for booleans, `CellSelectState::choose` for the dropdown).
- **`CellSelectState`** — a new caller-owned, total dropdown state (the
  `ScrollState`/`Selection` sibling: `open`/`close`/`move_highlight`/
  `reveal`/`choose`), fuzz-proved.
- **Any other widget** (Slider/Radio/DatePicker/custom) uses the existing
  `cell_rect` accessor — the ADR 0012 §1 escape hatch, total for any
  widget, no new contract. The pure-projection / single-reducer /
  no-callbacks / dependency-free invariants all still hold.

Demonstrated in the kitchen-sink **Data Grid** screen (an editable text
`name`, a `Select` `role`, a `Checkbox` `active`).

## Evidence

- `crates/rstui-widgets/src/tree.rs` establishes the precedent verbatim:
  "the reducer owns the tree … rebuilds a flattened `Vec` the widget
  reads." `project()` is that pattern generalized to filter/sort/group.
- `crates/rstui-widgets/src/scroll_view.rs` establishes "compose the
  caller-owned `ScrollState`, do not reinvent scrolling"; `DataTableState`
  follows it.
- `crates/rstui-widgets/src/input.rs` establishes the rendered-caret,
  borrowed-`TextEdit` editing model; the edited cell reuses `Input`
  directly, so there is exactly one caret implementation.
- ADR 0012 §1/§3 records pure geometry accessors (`SplitPane::divider_rect`,
  `ScrollView::viewport`) as *the* immediate-mode hit-test/event seam;
  `hit`/`cell_rect` are that seam for the grid.

## Consequences

**Positive**

- A spreadsheet-class grid with editing exists with **zero** new framework
  machinery: no retained tree, no `StatefulWidget`, no callbacks, no new
  `rstui-core` dependency. Every invariant of ADR 0002/0004/0012 holds.
- Fast scroll is structural (O(window)), not an optimization to maintain.
- The edit/sort/group/filter "hooks" are the ordinary reducer + pure
  accessors — the same pattern every other rstui widget already teaches.

**Negative / accepted**

- The caller must call `project()` in `update` on a data/spec change (the
  `Tree`-flatten cost). This is the deliberate price of a pure `view`; it
  is one documented line and is demonstrated in `data_table_demo`.
- Sort is lexicographic on cell text this slice (numeric/typed comparators
  deferred) — accepted and recorded, not a gap.

**Neutral / deferred**

- The §6 additives, each a future slice over this shape.
- ADR-number contention with parallel streams resolved by the
  `docs/merging.md` renumber-on-conflict rule (0013 was taken; this is
  0014).
