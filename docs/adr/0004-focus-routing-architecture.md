# ADR 0004: Focus-routing architecture

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

Three interactive form controls now ship in `rstui-widgets` —
`Checkbox` (iter 30), `Button` (iter 31), and `Radio` (iter 32). Each
renders a `focused: bool` visual through a shared `focus_style`
vocabulary, but **nothing in rstui decides which control is focused**,
how `Tab`/`Shift+Tab` moves focus, how a click sets it, or how a modal
traps it. Every one of those widget slices explicitly recorded the
focus manager as deferred: `checkbox.rs`'s module docs state *"This
widget renders a focused control; it does **not** decide which control
is focused. A focus manager — a registry of focusable widgets, how
Tab/arrow traversal works … is an expensive-to-reverse architectural
axis kept out of the widget slice."* Iterations 30, 31, and 32 each
closed by naming a "focus-routing ADR" as the next genuinely-new
surface, "best discharged as a decision-vs-mechanical-split
documentation ADR per the iter-10/19/21 precedent."

This is that record. The next two roadmap surfaces — `Input`/`TextArea`
(the first widget needing a cursor/text-edit model and the canonical
focus consumer) and the kitchen-sink demo (which must demonstrate
focus/keyboard/mouse interactions) — both depend on this decision, and
building either before it is fixed risks exactly the churn the
iter-10/19/21 precedent exists to prevent. Per that precedent, and
because `notes.md` is orchestrator-owned and cannot be written to, an
in-repo ADR that splits the decision from its mechanical execution is
the correct highest-leverage slice.

Constraints already locked in by earlier slices, which this decision
must fit rather than relitigate:

- **The view is pure and immediate-mode.** `App::view(&self, &mut
  Frame)` cannot mutate app state (iter 5); `Widget::render(self,
  area, buf: &mut Buffer)` is handed **only** a `Buffer` — never the
  `Frame`, never any ambient focus state (iter 7/28). There is **no
  retained widget tree**: each frame is reconstructed and diffed
  (iter 2/3). A widget physically cannot own focus, read who is
  focused, or self-mutate at render time. This is the dominant force.
- **`update` is the sole mutation point.** The Elm contract is pure
  `on_event(&self) -> Option<Message>` (intent) → `update(&mut self,
  Message) -> Cmd` (sole mutation) → pure `view(&self)` (iter 5). The
  reducer is deliberately the single testable source of truth; the
  runtime never silently swallows or reroutes input (a recorded
  iter-5 design property).
- **The pure-projection pattern is proven across the widget set.**
  List/Tabs (selection), Gauge (a scalar), Scrollbar (derived scroll
  metrics), Spinner (a tick), Table (2D selection), and
  Checkbox/Button/Radio (`focused`/`checked`/`selected`) are all pure
  projections of caller-owned model state. Iter 23 recorded that
  rstui's strict `view(&self)` makes ratatui's render-mutating
  `StatefulWidget` architecturally incompatible, and that *"whether
  rstui ever grows a stateful-widget/scroll-into-view seam is an
  expensive-to-reverse, ADR-worthy decision that must NOT be smuggled
  into a widget slice."* Focus is the next instance of that exact
  question.
- **Terminal focus already exists and is a different concept.**
  `rstui_core::event::Event::FocusGained`/`FocusLost`
  (`event.rs:291-294`, iter 4) model the **terminal window**
  gaining/losing OS focus. They are unrelated to *which widget inside
  the app* the keyboard is aimed at, and must not be conflated with
  it.
- **`rstui-core` is dependency-free and owns the shared seams** the
  runtime and apps both target — `event`, `event_source`, geometry,
  style, layout, the `Widget` trait (ADR 0002). `rstui-widgets`
  depends only on `rstui-core`; `rstui-runtime` depends only on
  `rstui-core`. Any focus primitive must respect these edges.
- The objective's roadmap includes select, input, textarea, trees,
  split panes, **modals**, **command palette**, forms — every one of
  which is a focus consumer, and modals/command-palette specifically
  require focus *capture and restore*.

## Decision drivers

1. **Pure-view compatibility** — focus must be expressible without
   any render-time mutation or ambient widget state; the
   `view(&self)`/`Widget::render(…, buf)` invariant is non-negotiable.
2. **Single source of truth** — exactly one place holds "who is
   focused"; `view` reads it, never a second copy.
3. **Single testable reducer** — focus transitions and input dispatch
   stay in `update`, TTY-free testable via `Harness`, never hidden in
   the runtime.
4. **No mandatory framework** — an app must be able to do focus with
   its own `enum` and zero framework types (the ratatui floor), and
   third-party widgets must need nothing new to participate.
5. **Totality** — any focus helper must be panic-free for any caller
   input (the iter-25 "a pure projection must be total" rule), since
   focus order/ids are caller-owned.
6. **Immediate-mode honesty** — no decision may require a retained
   focusable tree or engine-owned traversal rstui structurally does
   not have.
7. **Real-app sufficiency** — the model must support modal focus
   capture/restore, declarative modal input trapping, click-to-focus,
   and Tab traversal, because real TUIs (OpenCode) need them and they
   are the hardest pieces to retrofit.
8. **Reversibility** — focus shape constrains every future
   interactive widget and `Input`/`TextArea` specifically; fix it
   before more code assumes the wrong shape.
9. **Dependency-edge correctness** — any primitive must sit where
   both `rstui-runtime` and `rstui-widgets` can use it without a
   cycle and without polluting core's dependency-free guarantee.

## Options considered

### A. Runtime-owned focus manager (engine tracks focus, passes it to `view`)

The runtime holds the focused id and a focusable registry; `Tab` is
handled by the runtime; `view` is handed the focus state.

But: this creates a *second* source of truth outside the app's model,
defeats the single-testable-reducer property (focus transitions become
runtime behavior, not reducer behavior), and requires either passing
focus into `view` (a new ambient input that breaks "view is a pure
projection of the app's own model") or letting widgets read ambient
state (impossible — `Widget::render` gets only a `Buffer`). Works
against drivers 1, 2, 3. **Rejected.**

### B. Retained focusable tree with automatic traversal (OpenTUI/GPUI shape)

A focusable node tree the framework walks for `Tab` order and
hit-tests for click-to-focus, as OpenTUI's renderer and GPUI's
`window.focus_next()` do.

But: rstui is immediate-mode with **no retained tree** — `view`
produces a `Buffer`, not a node graph. Adopting this means building a
second retained-mode subsystem inside an immediate-mode framework
purely for focus, contradicting the entire iter-2/3 rendering model
and the proven pure-projection pattern. Works against drivers 1, 6.
**Rejected** as architecturally foreign.

### C. Per-widget owned focus with imperative `focus()`/`blur()` (Bubble Tea/OpenTUI shape)

Each widget owns a `focused` field and exposes `focus()`/`blur()`
methods that mutate it (and, in OpenTUI, wire key handlers).

But: rstui widgets are reconstructed every frame and handed only a
`Buffer`; they cannot own persistent state or mutate at render. This
is the render-mutating-`StatefulWidget` incompatibility (iter 23)
restated. Works against drivers 1, 2. **Rejected.**

### D. Focus is caller-owned model state, projected into pure-view widgets — **chosen**

Focus lives in the app's model as data, mutated only by `update`, read
by `view` to compute each widget's `focused: bool`. The framework adds
an **optional, pure, model-resident** focus-ring/registry primitive
that reduces boilerplate without owning state, plus a **decided but
mechanically deferred** model for modal focus scopes. This is the only
option consistent with every locked-in constraint, and it is exactly
what Checkbox/Button/Radio already assume — so it is largely the
*ratification of a forced choice*, with the genuine decisions being
the helper's shape, its crate home, and the deferred modal model.

## Decision

1. **Focus is caller-owned model state, never widget- or
   runtime-owned.** The single source of truth for "which widget is
   focused" is a value in the application's model, mutated only in
   `update`, read by `view` to pass `focused: bool` (or an id to
   compare) into each widget. This is *forced* by the pure-view /
   immediate-mode constraints, not a free choice; it is the exact
   contract `Checkbox`/`Button`/`Radio` already expose
   (`focused`/`focus_style`). Options A, B, and C are rejected. The
   framework **never** auto-tracks focus and **never** auto-routes
   input to "the focused widget."

2. **The zero-framework floor is ratified and permanent.** An app may
   model focus with its own `enum`/index and pass `focused: bool` into
   widgets with no rstui focus type at all (the ratatui `input-form`
   pattern). Third-party widgets participate by accepting a
   `focused: bool` builder and applying a `focus_style` last in their
   cascade — the existing Checkbox/Button/Radio contract, which is
   hereby the documented third-party focus-participation convention.
   No widget ever needs a framework focus type to be focusable.

3. **An optional, pure, model-resident focus model lands in a new
   dependency-free `rstui_core::focus` module.** It provides:
   - `FocusId`: an opaque, `Copy`, value-identity token (a newtype
     over a small integer). It is an *identity key the app mints*
     (const or via a counter), **not** a bool and **not** a
     window-backed handle — rstui has no window. Value identity
     (GPUI's `FocusHandle` lesson) makes "is this widget focused?" a
     cheap `==`, and lets a focus ring and a modal scope be expressed
     as ordered `FocusId`s.
   - `FocusRing`: a pure value type holding an **explicit ordered**
     `Vec<FocusId>` and the currently-focused id. It exposes
     `focus(id)`, `focused() -> Option<FocusId>`, `is_focused(id) ->
     bool`, and **pure, wrapping, total** `focus_next()`/
     `focus_prev()`. It lives as a *field in the app's model*, is
     mutated only by `update` (in response to `Tab`/`Shift+Tab`/click
     messages the app maps), and is read by `view` to compute each
     widget's `focused`. It is not runtime-owned, not ambient, not in
     the view's mutable path.
   - Focus **order is explicit data**, never derived from a tree
     (driver 6): rstui is immediate-mode, so the ring *is* the order.
     This is the load-bearing divergence from OpenTUI/GPUI and is
     recorded as deliberate.
   The module lands in `rstui-core` (not `rstui-widgets`) because it
   is a primitive with **no `Widget`/`Buffer` dependency** — it deals
   only in ids and ordering — and it is a shared seam both
   `rstui-runtime` (the deferred input-dispatch helper) and
   `rstui-widgets`/apps consume, exactly like `event` and
   `event_source`. It keeps core dependency-free.

4. **Input routing stays in the reducer; the runtime never
   auto-routes.** `App::on_event(&self)` continues to see every event;
   the app dispatches to the focused component by reading its own
   `FocusRing::focused()` in `update`. Click-to-focus is the app
   mapping a `MouseEvent` position to a `FocusId` in `update` against
   the `Rect`s it already computed during layout (a focus-region
   helper that records those rects is a *possible later additive*, not
   part of this decision — the area is app-owned). This preserves the
   iter-5 pure-intent / sole-mutation seam and the single-testable
   reducer (drivers 3) and is a deliberate divergence from
   OpenTUI/GPUI engine auto-routing, affordable because rstui's app
   owns the loop's semantics.

5. **Terminal focus and widget focus are explicitly separated.**
   `Event::FocusGained`/`FocusLost` (`event.rs:291-294`) remain
   **terminal-window** focus only and are documented as such; the new
   `FocusId`/`FocusRing` are **app/widget** focus. They never share a
   name or type. This is a recorded, deliberate divergence from Bubble
   Tea, whose *only* `Focus`-named type is the terminal one — a
   conflation its own example code makes confusing.

6. **The modal/scoped-focus model is DECIDED here but its mechanical
   landing is DEFERRED** to its own sequenced slice (decision-vs-
   mechanical split, per iter-10/19/21):
   - **Focus scopes are model state.** A modal/overlay pushes a
     `FocusScope` (a sub-set/sub-range of `FocusId`s) onto a
     model-owned stack in `update`; closing it pops the scope.
     `focus_next()`/`focus_prev()` are constrained to the active
     scope's slice (wrap within it), so the modal-containment logic
     that needs a global `FocusTrapManager` + bounded retry loop in
     gpui-component collapses to a one-liner because order is
     model-owned.
   - **Focus capture/restore is model state with validated restore.**
     Pushing a scope saves the previously-focused `FocusId`; popping
     restores it **only if that id is still registered** in the ring;
     a stale id (its widget gone while the modal was open) is ignored
     and focus falls back to the scope's first id. This is OpenCode's
     unmount-safe restore lesson expressed in the pure-model idiom (a
     tree-walk validation becomes a registry membership check).
   - **Modal input trapping is declarative gating in the reducer**,
     keyed on "is a scope active" — never a runtime input sink.
     Background key handling in `on_event`/`update` is predicated on
     the modal stack being empty, exactly as OpenCode disables every
     background binding layer while a dialog is open. The runtime
     stays a dumb pump; trapping is a reducer predicate, keeping it
     TTY-free testable.
   The `Modal` widget, the `FocusScope`/scope-stack helper, and any
   focus-region (area→`FocusId`) helper are **separate future
   slices**, each gated on a concrete consumer (the first is
   `Input`/`TextArea` needing `FocusRing`; modal scopes land with the
   first modal/command-palette widget). They are recorded here so
   those slices implement a decided model rather than re-deriving one.

## Evidence

Facts gathered by reading the reference projects locally; paths cited
so the reasoning is auditable.

**Bubble Tea** (`charmbracelet/bubbletea/main`) — Elm-architecture
analog:

- The **only** `Focus`-typed thing in the entire framework is
  terminal focus: `focus.go:5,9` — `type FocusMsg struct{}` /
  `type BlurMsg struct{}`, empty structs for the *terminal window*,
  opt-in via `ReportFocus` (`tea.go:163-170`). An exhaustive search
  for `FocusManager`/`FocusGroup`/`Focusable` across all `*.go`
  returns only that one hit. There is **no** framework focus manager,
  registry, or Tab-traversal helper.
- Component focus (bubbles `Focus()`/`Blur()`/`Focused()`) is real
  mutable component state driven imperatively by the parent; *which*
  child is focused is **100% hand-rolled in each app's `Update`**
  (`examples/focus-blur/main.go:32-35` keeps a `focused bool` in the
  app model). Direct support for Decision §1/§2: the ecosystem default
  is app-owned focus, no framework router — and §5: the name
  conflation is a real, citable confusion.

**ratatui** (`ratatui/ratatui/main`) — the closest Rust analog:

- **No focus system anywhere.** `ratatui-core` and `ratatui-widgets`
  contain zero focus traits/structs/state; an exhaustive `Focus`
  search across all `*.rs` finds only an *app-defined* `enum Focus`
  in an example and an unrelated doc comment.
- The canonical multi-field example
  `examples/apps/input-form/src/main.rs:81-151` stores focus as an
  app-owned `enum Focus { … }` field, traverses with a hand-written
  `const fn next()` round-robin, and `view` reads `self.focus` to
  place the cursor. Its own doc comment says to reach for
  `tui-input`/`tui-prompts`/`tui-textarea` — *the framework provides
  nothing*. `examples/apps/demo2/src/app.rs:18-215` does the same with
  a `from_repr` index. Direct support for Decision §1/§2 (forced
  app-owned focus) and the §3 observation that a reusable, total
  focus-ring primitive is exactly the boilerplate ratatui apps
  re-derive every time.
- `examples/concepts/state/src/bin/nested-stateful-widget.rs:96-103` —
  ratatui's `StatefulWidget::render` mutates `state` at render time
  (`*state += 1`). This is the pure-view incompatibility (iter 23)
  restated; it is why Option C is rejected, not copied.

**OpenTUI** (`anomalyco/opentui/main`) — most-adopted TUI, retained-mode:

- Focus is a mutable per-node boolean (`Renderable.ts:226-228`:
  `_focusable`/`_focused`/`_hasFocusedDescendant`); `focus()`
  (`Renderable.ts:390-421`) mutates `self`, **wires keypress handlers
  into a global bus**, and bubbles an ancestor flag; the renderer
  holds a single `_currentFocusedRenderable` pointer that auto-blurs
  the previous (`renderer.ts:1150-1167`). Click-to-focus walks the
  parent chain in the renderer (`renderer.ts:3038-3047`,
  `autoFocus` default true).
- There is **no `tabIndex`, no `focusNext`, no focus-order registry**
  anywhere (grep-confirmed); Tab traversal is hand-written by the app
  over its own ordered array (`packages/examples/src/input-demo.ts:304`).
- Direct support for Option B/C rejection (render-time self-mutation +
  global handler wiring are impossible under
  `Widget::render(…, buf)`), and for Decision §3's "order is explicit
  app data" — even the retained-tree framework makes the app own the
  order.

**gpui-component** (`longbridge/gpui-component/main`) — breadth analog,
GPUI:

- Focus is an opaque value-identity token `FocusHandle`
  (`focus_trap.rs:2`), created from `cx.focus_handle()`
  (`input/state.rs:404`), bound via `.track_focus(&handle)`
  (`button/button.rs:465`), queried at render with
  `focus_handle.is_focused(window)` (`button/button.rs:453`);
  `contains_focused` reports self-or-descendant
  (`focus_trap.rs:90`). Tab order is a numeric `tab_index`/`tab_stop`
  on the handle (`button/button.rs:357-366`); traversal is the GPUI
  *engine* built-in `window.focus_next(cx)`.
- The **only** framework focus manager is `FocusTrapManager`
  (`focus_trap.rs:53-58`), a global used *solely* for modal
  containment: `Root::on_action_tab` (`root.rs:414-482`) calls
  `window.focus_next` then loops up to `MAX_ATTEMPTS = 100` to keep
  focus inside the trap. Direct support for Decision §3 (adopt the
  opaque-identity-token idea as a plain `FocusId`, **reject** the
  window/engine backing rstui lacks) and §6 (model-owned order
  collapses gpui's global-manager + retry loop to a one-liner — the
  researched conclusion verbatim). `Styled::focus_ring(is_focused,…)`
  (`styled.rs:502-507`) confirms the "widget reads a focused bool and
  draws a ring" convention rstui already follows.

**OpenCode** (`anomalyco/opencode/dev`) — a substantial production TUI
(TypeScript/Solid on OpenTUI), grounding the §6 real-app model:

- Renderer-owned single focus with `autoFocus: false` (`app.tsx:128`)
  — the app drives every transition; **no app-level active-pane enum
  and no Tab traversal at all** (deliberate: one primary editor +
  scroll-via-keybind panes).
- A single global **modal stack with focus capture on open and
  validated restore on close** (`ui/dialog.tsx:66-203`): opening saves
  `renderer.currentFocusedRenderable` and blurs it (`:141-144`);
  closing **walks the live tree to verify the saved node still
  exists** before refocusing (`:78-93`) — explicitly to survive a
  node unmounted while the modal was open. Direct support for
  Decision §6's validated-restore rule (membership check is the
  pure-model equivalent of the tree-walk).
- Modal input trapping is **declarative**: every background binding
  layer is predicated on `dialog.stack.length > 0` /a reactive
  `matcher` (`context/command-palette.tsx:88-94`, `app.tsx:803-817`),
  not an imperative event sink. A reactive effect re-asserts the
  correct focus owner because async remounts invalidate one-shot
  focus calls (`component/prompt/index.tsx:698-708`). Click-to-focus
  is per-element (`component/prompt/index.tsx:1558`); text-edit keys
  apply only when an editor is focused (`keymap.tsx:138-141`). Direct
  support for Decision §4 (input routed by focus state, not
  broadcast) and §6 (declarative reducer-gated trapping, not a runtime
  sink).

## Consequences

**Positive**

- Every locked-in invariant is preserved: pure `view`, immediate-mode,
  single testable reducer, dependency-free core. Focus needs **zero**
  new runtime machinery and no change to the `Widget` trait,
  `Backend`, `EventSource`, `run`, or `Harness`.
- `Checkbox`/`Button`/`Radio` are validated as-shipped — their
  `focused`/`focus_style` API is exactly the chosen contract, so this
  ADR adds no widget churn and retroactively justifies the iter-30
  sub-vocabulary decision.
- `FocusRing` turns the boilerplate every ratatui multi-field app
  re-derives (`enum Focus` + hand-rolled `next()`) into one reusable,
  totality-guaranteed primitive — and because it is model-resident and
  pure, the gpui-component global-`FocusTrapManager`-plus-retry-loop
  modal problem collapses to constraining `focus_next` to a scope
  slice.
- The modal capture/restore/trapping model — the highest-value,
  hardest-to-retrofit real-app requirement (OpenCode evidence) — is
  decided now, so `Modal`/command-palette/`Input` slices implement a
  fixed model instead of each re-deriving one.
- `FocusId` value identity makes "is this focused?" a `==` and makes
  third-party widgets first-class focus participants with no new
  framework type required.

**Negative / accepted**

- rstui will not offer automatic tree-order Tab traversal or
  automatic click hit-testing (OpenTUI/GPUI conveniences). Accepted:
  both require a retained tree rstui structurally rejects (iter 2/3),
  and explicit model-owned order is the honest immediate-mode design
  and matches what even retained-mode OpenTUI apps hand-roll anyway.
- The app is responsible for mapping clicks to `FocusId`s using the
  `Rect`s it laid out. Accepted: the app already owns layout; a
  focus-region helper is a clean later additive, not a missing
  primitive.

**Neutral / deferred**

- `rstui_core::focus` (`FocusId`/`FocusRing`) implementation — the
  immediate next slice (Follow-up §1).
- `FocusScope`/modal focus-stack helper and the `Modal` widget —
  deferred to the first modal/command-palette consumer (Follow-up §3).
- Any area→`FocusId` focus-region helper — deferred until a concrete
  click-to-focus consumer needs it.
- Whether terminal `FocusGained`/`FocusLost` should also pause
  animations/cursors is an app concern, untouched here.

## Follow-up

This ADR discharges the iteration-30/31/32 focus-routing surface and
is the reference contract for the next slices, sequenced:

1. **(Next slice, on its own — the mechanical landing.)** Add
   dependency-free `rstui_core::focus` with `FocusId` and `FocusRing`
   (`focus`/`focused`/`is_focused`/total wrapping
   `focus_next`/`focus_prev`), root re-exports, crate-doc bullet,
   unit tests including a totality property (any sequence of
   next/prev/focus over any ring never panics and stays in-set), a
   doctest, and README wiring. No widget or runtime change.
2. **`Input`/`TextArea`** then lands as the first `FocusRing` consumer
   (and the first text-edit/cursor widget), with a deterministic
   example driving focus across two inputs via `Harness`.
3. **Modal / command-palette + `FocusScope`** lands the §6 model
   (model-owned scope stack, validated restore, reducer-gated
   trapping) with its first concrete consumer; the `Modal` widget and
   any focus-region helper are gated on that slice, not pre-built.
4. The kitchen-sink demo (standing steering note) becomes the visible
   harness exercising real Tab traversal, click-to-focus, and modal
   focus capture once §2/§3 exist.
