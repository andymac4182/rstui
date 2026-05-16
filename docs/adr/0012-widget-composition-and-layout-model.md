# ADR 0012: Widget composition and layout model

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

`rstui-widgets` now ships ~53 widgets (the core set, the
rich-rendering family, and the form/data/navigation/layout/overlay/
control families) plus the `rstui-core` layout, focus, and text
primitives. A standing product question has been implicit since
ADR 0002/0004 but never recorded as its own decision: **how do these
widgets compose into a real, full-screen application** — specifically an
opencode/pi-class AI-agent TUI (sidebar + a scrollable, streaming
markdown transcript + a multi-line composer + a status bar + a command
palette + a modal stack)?

The question is now forced for three reasons:

1. The user requires verified coverage of "everything needed to build
   something like opencode/opentui/gpui-component", and a **dynamic
   full-screen showcase** that proves the widget set composes.
2. An authoritative source deep-dive of `anomalyco/opentui` and
   `anomalyco/opencode` (fetched via `opensrc`, cross-checked against
   `crates/rstui-widgets/src/lib.rs`) has produced a precise gap list.
   It must be reconciled into a *recorded* model, not re-derived per
   slice.
3. opentui is a **retained-mode, Yoga-flexbox** engine; opencode is
   built directly on it. rstui is **immediate-mode, pure-projection**.
   Whether rstui's model can assemble the same application — and where
   its honest seams are — is an expensive-to-reverse architectural
   judgement that belongs in an ADR, per the iter-10/19/21 and
   ADR 0002/0004 decision-vs-mechanical-split precedent.

Constraints already locked in by earlier slices, which this decision
must fit rather than relitigate:

- **The view is pure and immediate-mode.** `App::view(&self, &mut
  Frame)` cannot mutate; `Widget::render(self, area, buf)` is handed
  only a `Buffer`. There is no retained widget tree; every frame is
  reconstructed and diffed (ADR 0002/0004; iter 2/3/5/7).
- **State is caller-owned; the reducer is the sole mutation point.**
  Every widget is a pure projection of model state the `update`
  function owns (List/Table/Tree selection+offset, Input/Editor's
  borrowed `TextEdit`/`TextArea`, Toast's message list, Modal/Drawer
  "open", Select/Menu "open/highlight", …).
- **Focus is caller-owned model state** via `rstui_core::FocusRing`
  with a model-resident modal **scope stack** (`push_scope`/
  `pop_scope`, validated capture/restore) — ADR 0004 §6.
- **`rstui-core` is dependency-free** and owns the shared seams
  (`Layout`/`Constraint`/`Direction`, `focus`, `text_edit`,
  `text_area`, geometry); widgets depend only on it (ADR 0002).
- The vague-name ban and the `cargo xtask ci` gate (fmt, clippy,
  test, rustdoc `-D warnings`, lint-names) apply to every slice.

## Decision drivers

1. **Immediate-mode honesty** — composition must not require a
   retained tree, a flexbox engine, or render-time mutation rstui
   structurally rejects.
2. **Single source of truth** — one model owns all state; `view`
   reads, never copies or mutates it.
3. **Real-app sufficiency** — the model must demonstrably assemble an
   opencode-shaped app, the hardest concrete target.
4. **No mandatory framework** — an app composes with plain `Layout`
   and its own `enum`s; framework types (`FocusRing`, `ScrollState`)
   are optional ergonomics, never required.
5. **Totality** — every composition primitive is panic-free for any
   caller input (tiny/zero/oversized areas, out-of-range state).
6. **Discoverability** — one documented, demonstrated pattern, not
   N per-widget conventions; agents and humans copy it.
7. **Reversibility** — fix the model and its extension seams before
   more code (and the flagship example) assumes the wrong shape.

## Options considered

### A. Adopt a retained widget tree + flexbox (the opentui/GPUI shape)

Build a `Renderable` graph with Yoga-style `flexDirection`/`flexGrow`/
`gap`/absolute positioning, engine-walked focus and hit-testing.

Rejected. It contradicts the entire iter-2/3/5/7 immediate-mode
rendering model and ADR 0004's recorded rejection of retained focus
trees. The deep-dive confirms opencode's layouts are almost entirely
`flexDirection:"column"/"row"` + `flexGrow:1` + `flexShrink:0` + a few
absolute overlays — **all expressible as nested `Layout` splits + an
overlay `Rect`** — so the engine is not *required* for the target app,
only convenient. Adopting it would be a second rendering subsystem for
no structural necessity.

### B. Pure-projection widgets + core `Layout` + `Rect`-accessor composition — **chosen**

Composition is: the app owns all state in its model; `view` splits the
screen with `rstui_core::Layout`/`Constraint`/`Grid`/`SplitPane`/
`Align`, and renders pure-projection widgets into the resulting
`Rect`s. Container/overlay widgets expose **pure geometry accessors**
(`Block::inner`, `SplitPane::split`, `Grid::cell`, `Form::layout`,
`Modal::area`/`inner`, `Select::panel`, `Popover::placement`,
`Accordion::layout`, `ScrollView::viewport`) returning the `Rect`s the
caller renders children into. Focus is a model-resident `FocusRing`
with a modal scope stack. This is the only option consistent with
every locked-in constraint, and the deep-dive's verdict is that it is
**structurally sufficient** to assemble an opencode-shaped app — the
focus/modal scope-stack is in fact a *strength* over opentui's
imperative `focus.blur()`/`refocus()`.

### C. B plus an ergonomic declarative combinator layer

B, plus builder combinators (a declarative pane/stack DSL over
`Layout`). Deferred, not rejected: the user explicitly chose "formalize
the existing model" over adding a combinator layer. Recorded as a
possible future additive, gated on real boilerplate pain the flagship
example surfaces — not built pre-emptively.

## Decision

1. **Composition is option B and is now the recorded, documented
   model.** `docs/composition.md` is its practical guide;
   `crates/rstui-widgets/examples/gallery.rs` is its executable,
   headless-testable proof — a dynamic full-screen app, driven by the
   public `rstui-runtime` `run`/`App` loop, exercising every widget and
   every layout primitive. The pure-projection + caller-owned-state +
   `Rect`-accessor + `FocusRing` pattern is **the** composition
   convention; third-party widgets participate by following it (accept
   caller state, expose pure accessors, never mutate at render).

2. **No retained tree, no flexbox engine, no render-time mutation —
   permanently.** This is the load-bearing divergence from
   opentui/GPUI and is recorded as deliberate. `gap`/`flexWrap`/
   absolute-positioning conveniences are replaced by explicit `Layout`
   composition and a `Flow`/wrap widget + a `Layout` spacing additive
   (Follow-up §P2), not an engine.

3. **The deep-dive's genuine seams are decided as bounded, pure
   extensions, not model changes.** rstui's model is sufficient
   *structurally*; the honest friction is concentrated and is
   discharged by these additive, caller-owned, total primitives —
   each its own sequenced slice, none altering the immediate-mode
   contract:
   - **Scroll/viewport ergonomics** — a `rstui_core::scroll::ScrollState`
     pure value type (offset + follow-tail) with total
     `clamp`/`at_end`/`scroll_by`/`on_content_change`
     (sticky-bottom-while-streaming)/`show` (scroll-into-view). The #1
     near-blocker for a faithful streaming transcript; `ScrollView`
     consumes it; the app still owns it. (Follow-up §P0.)
   - **`Editor::content_height(width)`** — a pure measurement accessor
     for composer auto-grow (`minHeight`/`maxHeight`). (§P0.)
   - **Editor/Input extmarks** — caller-owned atomic styled ranges the
     widget projects (the @-mention/paste "pills"); the reducer owns
     and re-derives them. (§P1.)
   - **A standalone `LineNumberGutter`** widget exposing an inner
     `Rect` (the `Block::inner` pattern). (§P1.)
   - **A `rstui_core` text-selection model** — caller-owned
     `Selection { anchor, active }` in content coordinates + a
     selected-cell projection + `selected_text()` extraction
     (drag-select → copy). Architecturally novel for immediate-mode;
     its own carefully-designed slice. (§P1.)

4. **Render-fidelity ceilings are accepted and recorded, not treated
   as gaps.** Per-language **syntax highlighting** is a *heavy alien
   dependency* by ADR 0002 §4 doctrine: it belongs in an **optional
   `rstui-syntax` sibling crate** or the generic zero-dependency
   tokenizer in `Markdown`/`Diff` is the accepted floor — never the
   dependency-free core. Fractional **alpha compositing** is emulated
   with caller-chosen blended colours over the opaque clear-region
   primitive. These are deliberate ceilings.

5. **Cross-stream seams are flagged, not built here.** **Diff
   split/side-by-side** and **Markdown `streaming`/`conceal`** belong
   to the rich-rendering stream (it owns `diff`/`markdown`); mouse
   **drag**/**bracketed-paste-payload** belong to the runtime/event
   stream; the keymap/leader/command-registry and plugin-slot systems
   are correctly **application** concerns, not widgets. Recorded so the
   flagship example uses the current widgets as-is and the owning
   streams pick these up.

## Evidence

Facts gathered by fetching the reference projects' actual source via
`npx opensrc@latest` and cross-checking
`crates/rstui-widgets/src/lib.rs`; cited so the reasoning is auditable.

- **opentui** (`anomalyco/opentui/main`) is retained-mode: every node
  is a `Renderable` (`packages/core/src/Renderable.ts:204`) with a Yoga
  node, z-index, opacity, and mouse/keyboard handlers; layout is
  flexbox (`LayoutOptions`, `Renderable.ts:63`). Its built-in
  renderable set (`packages/core/src/renderables/`: Box, Text/StyledText,
  Input, Textarea/EditBuffer, Select, TabSelect, Slider, ScrollBox,
  ScrollBar, Code, Markdown, Diff, TextTable, LineNumber, ASCIIFont,
  FrameBuffer) maps onto rstui's set with the residual gaps in
  Decision §3/§4 — **capability parity holds; the engine does not.**
- **opencode** (`anomalyco/opencode/dev`,
  `packages/opencode/src/cli/cmd/tui/`) is a production SolidJS TUI on
  `@opentui/solid`. Its app shell (`app.tsx:830`) is a
  `flexDirection:"column"` root → routed body → bottom strip, with a
  caller-owned dialog **stack** (`ui/dialog.tsx`: push/replace/clear,
  focus save/restore) and ~25 dialogs, a sticky-bottom streaming
  transcript `<scrollbox stickyStart="bottom">`
  (`routes/session/index.tsx`), and a `<textarea minHeight=1
  maxHeight=6>` composer with extmark pills
  (`component/prompt/index.tsx`). Every one of these decomposes onto
  nested `Layout` + `Rect` accessors + `FocusRing` scope stack — the
  deep-dive's structural verdict, recorded verbatim in Decision §1/§3.
- rstui's `FocusRing` scope-stack (ADR 0004 §6) is a *closer* fit to
  opencode's "blur prompt when a dialog opens, restore on close"
  (`ui/dialog.tsx refocus()`) than opentui's imperative model — a
  recorded strength, not parity.

## Consequences

**Positive**

- The composition model is recorded, documented (`docs/composition.md`),
  and *demonstrated* (`gallery.rs`, headless-testable) — the lowest-
  ambiguity instruction for humans and agents assembling rstui apps.
- The opencode/pi/gpui-component coverage question has an auditable,
  source-grounded answer; residual gaps are a bounded, prioritized,
  pure-extension list, not an open-ended unknown.
- Every extension in Decision §3 preserves the immediate-mode / pure-
  view / single-reducer / dependency-free invariants; none needs new
  runtime machinery or a `Widget`-trait change.

**Negative / accepted**

- rstui will not offer flexbox `gap`/`flexWrap`/absolute positioning,
  engine focus/hit-testing, true alpha, or built-in syntax
  highlighting. Accepted and recorded: each is either an explicit-
  `Layout` composition, a bounded additive (§3/§P2), or a deliberate
  ADR-0002 dependency ceiling (§4).
- ADR-number contention with parallel streams is possible; the
  `docs/merging.md` rebase-before-merge protocol resolves it (renumber
  on conflict).

**Neutral / deferred**

- The declarative combinator layer (Option C) — until real boilerplate
  pain justifies it.
- Cross-stream seams (Decision §5) — owned elsewhere.

## Follow-up

This ADR is the reference contract for the remaining sequenced slices:

- **§P0 (next):** `rstui_core::scroll::ScrollState` + `ScrollView`
  consumption + `Editor::content_height`. The faithful-transcript
  near-blocker.
- **§P1:** Editor/Input extmarks; standalone `LineNumberGutter`; the
  `rstui_core` text-selection model (its own slice).
- **§P2:** `Flow`/wrap widget + `Layout` spacing; `Table` rich
  cells/auto-fit; `Scrollbar` arrows; `Slider` vertical;
  `Block::bottom_title`.
- The flagship `gallery.rs` + `docs/composition.md` land with this ADR
  and are updated as §P0/§P1 widen what they can faithfully show.
- Cross-stream items (Decision §5) are recorded for the owning streams;
  not built here.
