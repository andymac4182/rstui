# Code review & code editing: capability review and roadmap

**Question this answers:** what editing/review widgets and primitives does
rstui have today, and what must be added to (a) support *everything vim does
around editing files* and (b) match a review-first diff viewer in the class of
[`modem-dev/hunk`](https://github.com/modem-dev/hunk)?

This is a scoping document, not an implementation. Every "have" row cites the
code it is grounded in (`file:line`); every "need" row states the *model
primitive or widget* required and where it slots, so the work can be sliced
into the parallel-stream waves at the end.

## TL;DR

- **Editing primitives we have are a solid floor, not a vim engine.**
  `TextArea` (`crates/rstui-core/src/text_area.rs:107`) is a total, caller-owned
  multi-line document with arrow/page/doc motions and char-level edits. It has
  **no** word/find/match motions, **no** span/linewise edits, **no
  undo/redo**, **no** logical selection, **no** registers/marks/search, and
  there is **no modal interpreter** anywhere. Roughly 80% of vim is the layer
  *above* `TextArea` that does not exist yet.
- **The diff widget is a strong single-patch renderer, not a review app.**
  `Diff` (`crates/rstui-widgets/src/diff.rs:238`) parses one unified patch into
  unified/split views with word-LCS and dependency-free syntax tinting. It has
  **no** multi-file changeset model, **no** per-line comment anchors, **no**
  fold/collapse, **no** hunk/file/comment navigation index, and is **read
  only** (no stage/unstage). hunk's *chrome* (sidebar, split/stack, filter,
  menus, help, status line) is already covered by existing widgets — the gap
  is the *review-domain* model and one stream widget, not the shell.
- **The architecture forces where every new piece goes** (next section). None
  of this is a widget owning state: vim modes, registers, undo, search, the
  changeset, and review comments are all **caller-owned model primitives**
  mutated only in `update`; widgets stay pure projections.

## The rule every new piece must obey

From [Architecture](architecture.md), [ADR 0004](adr/0004-focus-routing-architecture.md),
[ADR 0012](adr/0012-widget-composition-and-layout-model.md) and
[composition.md](composition.md), restated because it determines the entire
shape of the work below:

> The **model owns the state.** `view` is a **pure projection**; a widget is
> handed only a `Buffer` at render time, never mutates, never panics, holds no
> retained tree. `update` is the only place state changes. Hit-testing is the
> reducer's job against the `Rect`s it laid out.

Consequences that are not optional:

- **A "vim mode" is not a widget.** It is a pure finite-state machine in the
  reducer mapping `(mode, key, count, pending-operator)` to operations on
  caller-owned `TextArea` + registers + marks. The `Editor` widget only
  *renders* the resulting document, caret shape, and selection span. This is
  the same split `Input`/`TextEdit` and `Editor`/`TextArea` already use
  (`crates/rstui-core/src/text_area.rs:1`).
- **Selection, search matches, folds, comments are caller-owned span/flag
  state** the widget reads per cell at render (the model `Selection` already
  proves the pattern: `crates/rstui-core/src/selection.rs:142`,
  `contains()` at `:247`).
- **Side effects are `Cmd`s, not core.** System clipboard (`"+`/`"*`),
  `$EDITOR` launch, file-watch/reload, `git`/`jj` invocation, `:!filter` —
  none belong in the pure core. They are runtime commands; this doc names the
  seams but they are out of the dependency-free substrate.
- **Dependency-free below the backend** ([ADR 0002](adr/0002-widget-crate-boundary.md)).
  A real regex engine and a real syntax-tree highlighter are *optional,
  feature-gated* additions, never the default path. The floor is the
  hand-written scanner class `Diff`/`Markdown` already use.

---

## Part A — What we have today

### A.1 Editing primitives (`rstui-core`)

| Primitive | File | What it does | What it does **not** do |
|---|---|---|---|
| `TextEdit` | `text_edit.rs:91` | Single-line: value/cursor/len; `move_left/right/home/end`; `insert_char/str`; `delete_backward/forward` | No word ops, no undo, no selection. Fine as an ex/`:`/`/` command-line buffer |
| `TextArea` | `text_area.rs:107` | Multi-line doc (`Vec<String>` + `(row,col)` **char** cursor), sticky goal column, every method total. Motions: `move_left/right/up/down/home/end/doc_start/doc_end/page_up/page_down`. Edits: `insert_char/str/newline`, `delete_backward/forward` | **No** word/WORD/find/till/`%`/paragraph/sentence/screen motions; **no** span/linewise/join edits; **no undo/redo**; **no** selection, marks, registers, search |
| `Selection` | `selection.rs:142` | Row-major **content-buffer cell** (`Position`) span for mouse drag→copy over *rendered* output; `contains()`, `selected_text()` | Not logical `(row,col)` over the document; cannot express vim charwise/linewise/**blockwise** visual over `TextArea` |
| `ScrollState` | `scroll.rs` | Caller-owned scroll offset, clamp/`scroll_into_view` family | — (reused as-is) |
| `FocusRing` | `focus.rs` | Focus routing / hit dispatch | — (reused as-is) |

### A.2 Editing & review widgets (`rstui-widgets`)

| Widget | File | What it does | What it does **not** do |
|---|---|---|---|
| `Editor` | `editor.rs:118` | Pure projection of `&TextArea` + `focused` + caller-owned 2D scroll + extmarks + optional `Block`; renders its own block caret | Its own module doc defers **selection**, **undo**, **`scroll_into_view`** (`editor.rs:34`). No mode indicator, no visual-selection highlight, no search-match highlight, no relative-number/sign gutter |
| `Diff` | `diff.rs:238` | Read-only single unified-patch projection: unified + side-by-side split, line-number gutter, 3-colour scheme, intra-line word-LCS, optional dependency-free syntax tinting, 16-field `DiffTheme` (`diff.rs:121`), `scroll: u16` | **Single patch string only.** No multi-file changeset model, no per-line comment anchors, no fold/collapse, no hunk/file/comment navigation index, no selection, **read-only** (no stage/unstage/discard) |
| `LineNumberGutter` | `line_number_gutter.rs` | Aligned line-number column | No relative-number, no sign/fold column |

### A.3 Shell widgets already covering hunk's *chrome* (reuse, do not rebuild)

`Tree` (`tree.rs` — file explorer/outline), `Sidebar` (`sidebar.rs` — nav
rail), `SplitPane` (`split_pane.rs` — split/stack two-up), `CommandPalette`,
`Menu`, `StatusBar` (mode+path+message strip), `HelpOverlay` (`?` cheat
sheet), `ScrollView`+`Scrollbar`, `List`, `Table`, `Tabs`, `Input` (filter
box / command line), `Modal`, `Toast`, `Popover`, `Drawer`.

**Implication:** hunk's sidebar, split/stack/auto layout, file filter, menu
bar, help dialog and status line are *composition of widgets we already
ship*. The genuine gap is two domain models (vim interpreter; review
changeset+comments) plus a small number of widgets that project them — not a
pile of chrome.

---

## Part B — Target 1: everything vim does around editing

Vim decomposed into its real subsystems, each mapped to have / need. "Need →"
names the **model primitive** (caller-owned, mutated in `update`) or the
**widget** change, never "a stateful widget".

### B.1 Modal state machine — *missing entirely*

Normal, Insert, Visual (charwise), Visual-Line, Visual-Block, Replace,
Operator-Pending, Command-Line (`:`), Search (`/` `?`).

**Need →** an optional, dependency-free **vim interpreter** primitive: an
`EditorMode` enum + a pure reducer `step(state, key) -> effects` composing
`(count, operator, motion|text-object, register)` into operations on the
caller-owned `TextArea`/registers/marks. It is opt-in (an app may ignore it
and drive `TextArea` directly, exactly as today). Priority **P0** — nothing
else in Part B is usable without it.

### B.2 Motions — *basic only*

| Motion class | Have | Need |
|---|---|---|
| char/line/page/doc (`h j k l 0 $ G gg` arrows/PgUp/PgDn) | partial (`text_area.rs:330–426`; `0/^/$` and `gg/G` not distinct) | add `move_line_first_nonblank`, explicit `gg`/`G`, `\|` (to column), `_`, `+`/`-` |
| word/WORD (`w W b B e E ge gE`) | **none** | `TextArea` query methods returning the target `(row,col)` |
| find/till char (`f F t T ; ,`) | **none** | `TextArea::find_char(dir, ch, till) -> Option<(row,col)>` |
| match pair (`%`) | **none** | `TextArea::match_pair()` (bracket scan) |
| sentence/paragraph (`( ) { }`) | **none** | `TextArea` paragraph/sentence scan |
| screen (`H M L`), scroll-anchored (`zz zt zb`) | **none** | needs viewport in the reducer (already caller-owned scroll) |
| counts (`3w`, `d2j`) | **none** | count is interpreter state, applied to any motion |

**Need →** motions live as **pure query methods on `TextArea`** (return a
target position, mutate nothing) so vim *and* non-vim callers reuse them; the
interpreter just sequences them. Priority **P0** (word/find/`%`/counts), **P1**
(sentence/paragraph/screen).

### B.3 Text objects — *missing entirely*

`iw aw iW aW i" a" i' i\` i( a( i{ a{ i[ a[ i< a< it at ip ap is as`.

**Need →** a `text_object.rs` resolver: `(kind, inner|around, &TextArea,
cursor) -> Option<Span>` where `Span` is a logical `(start,end)` over the
document. Operators consume a `Span` whether it came from a motion or an
object. Priority **P1**.

### B.4 Operators & shortcuts — *missing entirely*

Operators `d c y > < = gu gU g~ !`; doubled `dd cc yy >> ==`; shortcuts
`x X s S r R ~ J gJ D C Y p P`. Linewise vs charwise vs blockwise semantics.

**Need →**
1. **`TextArea` span/linewise edits** (the missing edit primitives):
   `delete_span`, `replace_span`, `delete_lines`, `join_lines`,
   `shift_lines(±)`, blockwise insert/delete. Total, like the rest of
   `TextArea`.
2. **Operator+target composition** in the interpreter, writing yanked text to
   a register before deleting/changing.

Priority **P0** (the span edits + `d/c/y/x/p/dd/yy/J` — the daily core),
**P1** (`> < = ~ gu/gU`, blockwise).

### B.5 Registers & clipboard — *missing entirely*

Unnamed `""`, numbered `"0`–`"9`, named `"a`–`"z` (append `"A`–`"Z`),
read-only `". "% ": "/`, black-hole `"_`, system `"+ "*`. Charwise vs
linewise vs blockwise paste.

**Need →** a `registers.rs` primitive (a small typed map + paste-type tag).
System-clipboard read/write is a **`Cmd` seam**, not core (per the rule).
Priority **P0** (unnamed + numbered + named + linewise/charwise paste), **P1**
(system clipboard `Cmd`, blockwise).

### B.6 Undo / redo — *missing entirely, highest-risk gap*

`u`, `Ctrl-R`, `U`, `.`-aware change grouping. Vim also has an **undo tree**
(`g-` `g+` `:earlier` `:later`).

**Need →** an undo model for `TextArea`: snapshot-or-change-list with
keystroke **coalescing** (one `u` undoes an insert run, not one char). This is
the **single biggest correctness gap** — editing without trustworthy undo is
not shippable. Ship **linear undo/redo first** (P0); the full **undo tree +
time-travel is an explicit later** (P2, see Non-goals).

### B.7 Search — *missing entirely*

`/ ? n N * #`, incremental (`incsearch`), highlight (`hlsearch`), `smartcase`,
offsets, `gd`.

**Need →** a `search.rs` query over `TextArea` returning match `Span`s; the
`Editor` renders matches as a styled span set (same per-cell `contains`
pattern as `Selection`). Regex: ship **literal + a documented small
"very-magic" subset** dependency-free; a real regex engine is an *optional
feature-gated crate* ([ADR 0002](adr/0002-widget-crate-boundary.md)).
Priority **P1**.

### B.8 Marks, jumps, dot-repeat, macros — *missing entirely*

`m{a-z}` `` `{a-z} `` `'{a-z}` `'' `` `` `` ``; jumplist `Ctrl-O`/`Ctrl-I`;
change list `g;`/`g,`; dot-repeat `.`; macros `q{reg}` `@{reg}` `@@`.

**Need →** `marks.rs` (named positions + jumplist + change list) and a
**last-change / keystroke recorder** in the interpreter (dot-repeat and
macros are the same recording mechanism). Priority **P1** (marks, `.`), **P2**
(jumplist, macros).

### B.9 Ex command line — *missing the parser*

`:` ranges (`. $ % 'a,'b /pat/`), `:w :q :wq :x :q!`, `:s/old/new/[g/c]`,
`:g/pat/cmd`, `:%!cmd`, `:e :noh :set :map`.

**Need →** a `:`/`/` line-input mode (the **`TextEdit` buffer is reused** —
no new edit widget) + a small ex parser in the interpreter, mapping the
**editor-relevant subset** to operations/`Cmd`s. Full Ex/Vimscript is a
non-goal. Priority **P1** (`:w :q :s :noh :set`), **P2** (`:g`, ranges,
`:map`).

### B.10 Widget surface for editing — *gaps in `Editor`*

`Editor` must additionally **render** (never own): the visual-selection span
(charwise/linewise/blockwise), search-match highlight, a caret *shape* per
mode (block/bar/underline), a relative-number + sign/fold gutter, and provide
the explicitly-deferred `scroll_into_view` helper (`editor.rs:34`). A
`StatusBar` composition shows mode/`:`/`/` line — no new widget needed there.
Priority **P0** (selection + caret shape + `scroll_into_view`), **P1**
(search highlight, relative-number/sign gutter).

### B.11 Out of the *framework* (app composition, not new widgets)

Window/buffer/tab management (`:sp :vsp Ctrl-W` `:bn` `:tabnew`, netrw) is
**composition over existing `SplitPane`/`Tabs`/`Tree`** plus app state — a
worked *example*, not new widgets. Stated as a non-goal for the widget layer
so it does not creep into the core slices.

---

## Part C — Target 2: hunk-grade review-first diff viewer

hunk's surface, decomposed from its source (multi-file review stream, sidebar,
split/stack/auto, inline agent/AI/user annotations, hunk/file/comment
navigation, fold, filter, watch, themes, pager, `$EDITOR`). Mapped to
have/need.

### C.1 Multi-file changeset model — *missing*

hunk reviews a *changeset* of many files, each with status (added / deleted /
renamed / binary / untracked / too-large), `+/-` stats, ordered hunks.
`Diff` parses exactly **one** patch string and exposes no structure.

**Need →** a `changeset.rs` model: `Changeset → DiffFile → Hunk` with stats +
status + an **ordered file/hunk index** (the substrate every navigation key
needs). `Diff`'s hand-written unified-diff scanner (`diff.rs:1`) is the
reusable basis — **extract/extend it, do not duplicate the grammar**.
Priority **P0** for review.

### C.2 Review-stream widget — *missing*

hunk's main pane is one top-to-bottom stream of *all* visible files' diffs
with file headers, per-file and per-hunk **fold**, hunk-header toggle,
line-wrap toggle, horizontal code scroll, line-number toggle. `Diff` renders
one patch with no fold and no stream.

**Need →** a `review_stream.rs` widget: pure projection of a `&Changeset` +
caller-owned view state (scroll, fold set, selected file/hunk, the
toggles) reusing `Diff`'s row renderer. Priority **P0** for review.

### C.3 Inline annotations / review comments — *missing*

hunk interleaves agent/AI/user notes anchored to `(path, side, line)`,
expandable note rows, a draft-note composer, comment list, and `{`/`}`
comment navigation (`src/ui/hooks/useAppKeyboardShortcuts.ts`).

**Need →** an `annotations.rs` model (comments anchored by `(path, side:
old|new, line)` + source `ai|agent|user` + an ordered comment index for
`{`/`}`) and the stream widget's **note-row rendering**. The draft composer
**reuses `Editor`** — no new edit widget. Priority **P0** (model + render +
nav), **P1** (draft compose/save/cancel flow as an example).

### C.4 Navigation — *index is the gap, not the state*

`[`/`]` hunk, `,`/`.` file, `{`/`}` comment, `gg/G`, half/page, `←/→`
horizontal. The *position* is already caller-owned scroll/selection (we have
that); what is missing is the **ordered index** to compute "next hunk" — i.e.
C.1's model. Once C.1 exists this is reducer arithmetic. Priority falls out of
C.1.

### C.5 Layout, filter, themes, menus, help, status — *reuse*

split/stack/auto → `SplitPane` + a width-breakpoint the **reducer** picks
("auto" is not a widget, it is `if width < N { stack } else { split }`,
documented as a pattern). Filter → `Input`. Themes → `DiffTheme`
(`diff.rs:121`). Menus → `Menu`/`CommandPalette`. Help → `HelpOverlay`.
Status/mode line → `StatusBar`. **No new widgets.** Priority: example glue.

### C.6 Watch / reload / `$EDITOR` / pager / VCS — *`Cmd` seams*

Watch-reload, `e` open-in-`$EDITOR`, pager mode, and getting the diff from
`git`/`jj` are **side effects = runtime `Cmd`s**, not core/widgets. This doc
names them as seams; they are app/runtime work, demonstrable in the example.

### C.7 Editable review: stage / unstage / discard hunk — *stretch*

hunk itself is review-only, but "code editing" implies the `git add -p`
class: a diff whose hunks are *selectable and stageable*. This is a larger
interactive widget (selectable hunk spans + a staged/unstaged flag set on the
changeset model). Priority **P2 / stretch**, explicitly flagged so it does not
block the review-only P0.

---

## Part D — Consolidated roadmap

Sliced to honour the parallel-stream discipline ([merging.md](merging.md)):
**new files wherever possible** (conflict-free); the only shared-file edits
are additive method-sets on `TextArea` (serialise if a stream is mid-flight in
`text_area.rs`). Waves are dependency-ordered; items within a wave are
mutually independent and parallelisable.

### Wave E1 — editing-core foundations (`rstui-core`)

| Slice | File | Gap closed | Pri |
|---|---|---|---|
| E1a | `text_area.rs` (+methods) | word/WORD, find/till, `%`, para/sentence, `0/^/$`, `gg/G`, count-ready motions (B.2) | P0 |
| E1b | `text_area.rs` (+methods) | span/linewise edits: delete/replace span, delete/join/shift lines (B.4.1) | P0 |
| E1c | `undo.rs` (new) | linear undo/redo with keystroke coalescing (B.6) | P0 |
| E1d | `text_object.rs` (new) | text-object → logical `Span` resolver (B.3) | P1 |
| E1e | `doc_selection.rs` (new) | logical charwise/linewise/blockwise selection over `TextArea`, distinct from cell `Selection` (B.10) | P0 |

### Wave E2 — vim interpreter (`rstui-core`, optional & dependency-free)

| Slice | File | Gap closed | Pri |
|---|---|---|---|
| E2a | `registers.rs` (new) | register file + paste-type; clipboard `Cmd` seam (B.5) | P0 |
| E2b | `vim/mod.rs` (new) | `EditorMode` + `(mode,key,count,operator,target)` FSM, dot-repeat recorder (B.1, B.4.2, B.8 dot) | P0 (sub-slice) |
| E2c | `search.rs` (new) | literal + small "very-magic" match-span query (B.7) | P1 |
| E2d | `marks.rs` (new) | marks + jumplist + change list (B.8) | P1 |
| E2e | `vim/ex.rs` (new) | `:` line mode (reuse `TextEdit`) + editor-relevant ex subset (B.9) | P1 |

### Wave E3 — editing widget surface (`rstui-widgets`)

| Slice | File | Gap closed | Pri |
|---|---|---|---|
| E3a | `editor.rs` | render selection span + per-mode caret shape + `scroll_into_view` (B.10, the deferred trio) | P0 |
| E3b | `editor.rs` / gutter | search-match highlight + relative-number/sign/fold gutter (B.7, B.10) | P1 |
| E3c | example (new) | a vim-grade editor example proving the Elm loop end-to-end | P1 |

### Wave R1 — review model & stream (`rstui-widgets`, all new files)

| Slice | File | Gap closed | Pri |
|---|---|---|---|
| R1a | `changeset.rs` (new) | multi-file Changeset/DiffFile/Hunk + status + stats + ordered index, reusing `Diff`'s scanner (C.1) | P0 |
| R1b | `review_stream.rs` (new) | multi-file fold-able review-stream projection of a `&Changeset` (C.2, C.4) | P0 |
| R1c | `annotations.rs` (new) | comment/agent-note model + note-row rendering + comment index (C.3) | P0 |

### Wave R2 — review shell & seams (composition / examples / docs)

| Slice | File | Gap closed | Pri |
|---|---|---|---|
| R2a | example (new) | hunk-style review app: Sidebar/Tree + SplitPane(+auto breakpoint) + review_stream + Input filter + StatusBar + HelpOverlay + Editor draft note (C.5) | P1 |
| R2b | `review_stream.rs` | selectable/stageable hunks — `git add -p` class (C.7) | P2 |
| R2c | doc (this file / runtime.md) | watch/reload, `$EDITOR`, system-clipboard, pager `Cmd` seams (B.5, C.6) | P1 |

**Critical path to "usable":** E1a+E1b+E1c+E1e → E2a+E2b → E3a gives a
trustworthy modal editor. R1a → R1b → R1c gives a hunk-class review pane.
Those two chains are independent and can run as parallel streams.

---

## Part E — Explicit non-goals (so scope does not creep)

- **Vim window/buffer/tab/netrw management** — app composition over existing
  `SplitPane`/`Tabs`/`Tree`, an example, not framework widgets (B.11).
- **A full regex engine** — ship literal + a documented small subset
  dependency-free; a real engine is an *optional feature-gated crate*
  ([ADR 0002](adr/0002-widget-crate-boundary.md)).
- **Vim's full undo *tree* + `:earlier/:later` time-travel** — linear
  undo/redo ships first (B.6); the tree is a later, opt-in extension.
- **Ex/Vimscript completeness** — only the editor-relevant `:` subset (B.9);
  `:map`, scripting, autocommands are out.
- **Syntax-tree / LSP-grade highlighting** — the dependency-free tokenizer
  (`diff.rs` syntax mode) is the floor; tree-sitter is a feature-gated
  optional, never the default.
- **Being a VCS** — `git`/`jj` invocation is a `Cmd`/app concern; the
  framework parses patches and projects them, it does not run version
  control.

## See also

- [code-editor-and-diff-deep-dive.md](code-editor-and-diff-deep-dive.md) —
  the companion deep dive: deepens this roadmap's scrolling (`E3a`) and
  select-then-replace (`E1b/E1e/E3a`) threads, **revises** the
  syntax-highlighting non-goal below into a specified two-tier design
  ([ADR 0022](adr/0022-syntax-colour-and-symbol-engine.md)), adds a
  symbol/outline panel, and audits every other `Editor`/`Diff` gap.
- [Architecture](architecture.md) · [ADR 0004](adr/0004-focus-routing-architecture.md)
  · [ADR 0012](adr/0012-widget-composition-and-layout-model.md) ·
  [composition.md](composition.md) — the pure-projection rule the whole
  roadmap is shaped by.
- [Component library](widgets/README.md) — the shell widgets Part C reuses.
- [Core reference](core-reference.md) — where the new `rstui-core` primitives
  (Wave E1/E2) document themselves.
- [Merging](merging.md) — the parallel-stream protocol the wave slicing obeys.
