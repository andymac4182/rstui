# ADR 0002: Widget crate boundary

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

Two user steering questions have been open since iteration 17 (and
deliberately deferred through iteration 18). They ask, precisely:

1. Is `rstui-core` the right home for concrete widgets such as
   `Paragraph`, or should widgets live in a separate crate such as
   `rstui-widgets`? Judge it on dependency boundaries, public API
   stability, third-party widget authoring, documentation /
   discoverability, test ergonomics, and whether core should stay
   focused on primitives (buffer, style, layout, terminal, events,
   widget *traits*).
2. Should each widget become its own crate, or be grouped into one or
   more component crates? Do **not** assume one-crate-per-widget;
   compare it against a grouped `rstui-widgets`, domain packs, and
   optional feature modules, judged on build times, incremental
   compilation, dependency isolation, feature flags, API
   discoverability, docs cohesion, versioning overhead, reusability,
   and agent ergonomics.

The user requires the recommendation and reasoning recorded **before
any mechanical move**. The orchestrator-owned `notes.md` cannot be
written to, so — per the precedent set by ADR 0001 and explicitly
validated as the correct highest-leverage move for a deferred steering
item whose decision is expensive to reverse — this ADR is that record.

Constraints already locked in by earlier slices, which this decision
must fit rather than relitigate:

- `rstui-core` is dependency-free and pure. It owns the `Widget`
  trait (iter 7), `Buffer`/`Cell`, geometry, style, stylize, layout,
  terminal, event, event_source, and the `Span`/`Line`/`Text` model.
- `rstui-core` *also* currently holds the only two concrete widgets —
  `Block` and `Paragraph` — in `crates/rstui-core/src/widget.rs`
  (~1183 LOC), together with `Alignment`, `Borders`, `BorderType`,
  `BorderSet`, `Padding`, and `Wrap`, all re-exported at the crate
  root.
- The single bounds-safe cell-stamping path is the free function
  `widget::set_cell`, currently `pub(crate)` and used by **both**
  `widget.rs` and `text.rs`. Being crate-private, it is *unreachable*
  by any out-of-crate widget — third-party widgets have no
  cell-stamping contract at all today.
- The objective's widget roadmap is ~25 widgets (text, labels, input,
  textarea, select, checkbox, radio, buttons, lists, tables, trees,
  tabs, split panes, modals, command palette, status bars, spinners,
  progress, notifications, forms, markdown/code, logs, inspector).
  Some (markdown/code, image/sixel) will eventually need optional,
  heavy transitive dependencies that must **never** be able to leak
  into a dependency-free core.
- A standing steering note makes third-party widget/component
  authoring a **first-class design goal**: developers *and coding
  agents* must be able to build, compose, document, and TTY-free-test
  their own widgets easily.
- Versioning is pre-1.0 (`0.0.1`). Breaking `rstui-core`'s public
  surface is cheapest now and gets strictly more expensive every
  iteration another widget accretes in core.

## Decision drivers

1. **Core API stability** — `rstui-core` is the universally
   depended-on crate (runtime, crossterm, every app and every widget
   crate transitively); its surface must be small and slow-moving.
2. **Core purity** — dependency-free, permanently; no widget
   dependency may ever threaten that.
3. **Third-party + agent widget authoring** — the cell-stamping
   contract must be *public*, and the first-party widget set must
   model the exact pattern a third-party crate follows.
4. **Discoverability & docs cohesion** — one obvious import and one
   cohesive docs surface for the widget set.
5. **Versioning & maintenance overhead** — minimize the number of
   crates published and versioned in lockstep across a ~25-widget
   roadmap.
6. **Build time & incremental compilation** — avoid micro-crate graph
   overhead; keep single-widget rebuilds cheap.
7. **Dependency isolation** — a widget that pulls a heavy or alien
   transitive dependency must not tax every downstream build.
8. **Test ergonomics** — the deterministic `TestBackend`/`Buffer`
   snapshot story must be unchanged for first- *and* third-party
   widgets.
9. **Reversibility** — the boundary is a breaking public-API change;
   decide and act before more code depends on the wrong path.

## Options considered

### A. Keep concrete widgets in `rstui-core`

Zero new crate; `Block`/`Paragraph` and the `Widget` trait
co-located.

But: core stops being "primitives only"; every widget add churns the
most-depended-on crate's version; a future heavy-dependency widget
either breaks core purity or becomes impossible to add; `set_cell`
stays crate-private so third-party widgets have *no* cell-stamping
contract. Works against drivers 1, 2, 3, 9. **Rejected** for the long
term (it is only the temporary status quo).

### B. One crate per widget (`rstui-list`, `rstui-table`, …)

Maximal per-widget dependency isolation and independent versioning.

But: ~25 crates to version, publish, and changelog in lockstep;
discoverability collapses (which crate is `List`?); cross-widget
reuse (Table↔List scroll/selection, Form↔Input/Select/Checkbox, a
shared word-wrap/reflow helper) is forced through premature *public*
inter-crate APIs — the exact coupling a boundary should reduce;
net-slower clean builds from per-crate fixed overhead and a wide
graph; worst-case agent ergonomics. **Rejected.**

### C. One grouped `rstui-widgets` crate, one module per widget — **chosen**

A single crate depending only on `rstui-core`, one module per widget.

Single discoverable import and one cohesive docs.rs surface; one
version to bump for widget changes; cross-widget reuse stays
intra-crate with no premature public seams; single-widget rebuilds are
module-granular and cheap (rustc incremental is module-grained);
depends only on `rstui-core`, so the `TestBackend`/`Buffer` snapshot
test story is unchanged. This is exactly `ratatui-widgets`' proven
shape.

### D. Domain-pack crates (`rstui-widgets-forms`, `rstui-widgets-data`, …)

Sensible *later* if a domain develops a heavy shared dependency
footprint; premature now (no domain has one). Folded into the chosen
option as a *future trigger*, not an initial split.

### E. Optional feature modules within one crate

Not a rival to C but the correct *dependency-isolation policy within*
C: gate a widget behind a Cargo feature only when it adds a transitive
dependency. Adopted as policy (see Decision §4).

## Decision

1. **Extract a single `rstui-widgets` crate** that depends only on
   `rstui-core` and holds the concrete widgets, **one module per
   widget**. It ships `Block` and `Paragraph` (with `Alignment`,
   `Borders`, `BorderType`, `BorderSet`, `Padding`, `Wrap`) on day one
   — a justified non-placeholder boundary, exactly as `rstui-crossterm`
   was justified by one real tested API before it grew. Concrete
   widgets do **not** live in `rstui-core` long-term.
   One-crate-per-widget is **rejected**.

2. **`rstui-core` keeps the `Widget` trait and all primitives**
   (geometry, style, stylize, layout, buffer, terminal, event,
   event_source, text). The `Span`/`Line`/`Text` model is a
   *primitive* and stays in core (it is foundational, `stylize` and
   `Block`'s title already depend on it, and ratatui keeps text in
   `ratatui-core`). The `Widget` trait stays in core because **every**
   widget crate — first- and third-party — implements
   `rstui_core::Widget`.

3. **The bounds-safe single-cell write becomes a public `Buffer`
   method in `rstui-core`**, consolidating today's `pub(crate)` free
   function `widget::set_cell` and named to sit in the existing
   `Buffer::set_str` / `Buffer::set_style` family. This is the
   explicit, *public* cell-stamping contract third-party widget
   authors build on, and it is precisely what makes the extraction
   possible without duplicating the one bounds-safe path. `text.rs`
   (staying in core) and `rstui-widgets` both go through this single
   method. This is the **one deliberate public-surface addition** the
   decision entails — and it is a first-class feature, not a reluctant
   leak: it directly discharges the third-party-authoring steering
   goal.

4. **Dependency-isolation policy inside `rstui-widgets`:** a widget is
   unconditionally compiled **unless** it introduces a transitive
   dependency, in which case it is gated behind a Cargo feature named
   for the widget. This is ratatui's *sole* rule (only `calendar` is
   gated, solely because it pulls `time`). A widget/domain whose
   dependency is heavy, optional, **and** conceptually alien (a
   browser/image/large-grammar engine) instead gets its **own crate**
   (the `gpui-wry` precedent), decided when that concrete dependency
   appears — never pre-emptively. **No per-widget feature flags by
   default.**

5. **An umbrella `rstui` crate** (re-exporting `rstui-core` +
   `rstui-widgets` + a default backend, feature-gating backends and
   dependency-bearing widgets) is the eventual top-level entry point
   but is **deferred**. It becomes justified when there is a *second
   backend* or a *feature-gated widget* to actually gate — the reason
   ratatui's umbrella exists. Not scheduled; recorded so "how do apps
   eventually depend on rstui without naming N crates" has an answer.

## Evidence

Facts gathered from the reference projects (read locally; paths cited
so the reasoning is auditable):

**ratatui** (`ratatui/main`) — the closest analog, validating this
exact shape at scale:

- Workspace splits `ratatui-core` (traits + buffer + layout + style +
  text + backend trait + terminal) from `ratatui-widgets` (**all**
  concrete widgets) from per-backend crates from an umbrella
  `ratatui`. `ratatui-core/Cargo.toml`'s own description is
  authoritative: *"Core types and traits for the Ratatui Terminal UI
  library. **Widget libraries should use this crate.**"* — i.e. core
  is what widget libraries (first- *and* third-party) depend on;
  concrete widgets are not in core.
- `Widget`/`StatefulWidget` live in `ratatui-core`
  (`ratatui-core/src/widgets.rs`); concrete widgets are **one crate**,
  `ratatui-widgets`, with **one module per widget**
  (`ratatui-widgets/src/lib.rs`: `block`, `paragraph`, `list`,
  `table`, `tabs`, `chart`, `canvas`, `barchart`, `gauge`,
  `sparkline`, `scrollbar`, `clear`, `fill`, …) — **not**
  one-crate-per-widget. A shared `reflow` helper is an internal module
  of that one crate.
- `ratatui-widgets/Cargo.toml` `[features]`: the **only** widget gated
  by a feature is `calendar`, with the verbatim rationale *"Widgets
  that add dependencies are gated behind feature flags to prevent
  unused transitive dependencies"* (`calendar` pulls `time`). Every
  other widget is unconditionally compiled. This is the precise rule
  adopted in Decision §4.
- `ratatui-widgets` reaches directly into `ratatui-core`'s **public**
  `Buffer` API to stamp cells (`paragraph.rs`/`block.rs`:
  `use ratatui_core::buffer::Buffer; buf.set_style(...)`,
  `cell.set_symbol(...)`). The public `Buffer`/`Cell` API *is* the
  contract the separate widgets crate (and third-party widget crates)
  build on — direct evidence for Decision §3.
- The umbrella `ratatui` re-exports core + widgets + backends and
  feature-gates them (`default = ["crossterm", …, "all-widgets",
  …]`; `crossterm`/`termion`/`termwiz` one feature per backend;
  `widget-calendar → ratatui-widgets/calendar`) — the model deferred
  in Decision §5.

**gpui-component** (`longbridge/gpui-component`) — breadth analog:

- The component library is **one crate** (`gpui-component`, ~55
  component modules: `button`, `input`, `list`, `table`, `tree`,
  `form`, `select`, `checkbox`, `radio`, `tab`, `dialog`,
  `notification`, `progress`, `spinner`, `chart`, `text`
  (markdown), …) depending on the separate upstream `gpui` primitives
  crate — the same primitives-crate ← one-grouped-components-crate
  shape, **not** per-component crates.
- Heavy components (markdown, code editor, charts) are *always*
  compiled; only the ~33 Tree-Sitter *language grammars* are
  per-language feature-gated (each a separate heavy dep), and
  *webview* — a heavy, optional, conceptually alien dependency (a
  browser engine) — is split into its own separate crate `gpui-wry`
  that the component crate does not depend on. Direct precedent for
  Decision §4's "feature-gate on dependency footprint; own-crate only
  for a heavy alien dependency".

## Consequences

**Positive**

- `rstui-core` becomes primitives-only and version-stable; widget
  churn no longer forces core version bumps across the whole
  ecosystem.
- The first-party `rstui-widgets` crate becomes the *worked reference*
  for third-party widget crates — depend on `rstui-core`, `impl
  rstui_core::Widget`, stamp through the public `Buffer` method,
  TTY-free snapshot-test against `TestBackend` — which
  *operationalizes* the first-class third-party-authoring steering
  goal instead of merely asserting it.
- One discoverable import and one cohesive docs.rs surface for the
  widget set; lowest-ambiguity instruction for agents generating TUIs.
- Future heavy-dependency widgets are isolatable by a Cargo feature
  or, at the extreme, their own crate — *without ever* threatening
  core purity, because the dependency can only ever enter
  `rstui-widgets` or a leaf crate.
- The shared word-wrap/reflow logic and future cross-widget reuse
  (Table↔List, Form↔Input) stay intra-crate, with no premature public
  inter-crate API.

**Negative / accepted**

- Moving `Block`, `Paragraph`, `Alignment`, `Borders`, `BorderType`,
  `BorderSet`, `Padding`, and `Wrap` out of `rstui-core`'s root
  re-exports is a **breaking change** to `rstui-core`'s public API
  (examples and tests import them from core today). Accepted: the
  project is pre-1.0, and the cost only grows with every widget added
  to core, so the extraction is scheduled as the **immediate next
  slice**.
- One more crate to build and version. Accepted: it is the *only*
  widgets crate, replacing N-crate overhead, and pays for itself the
  moment a third widget lands.

**Neutral / deferred**

- The umbrella `rstui` crate (Decision §5).
- Domain-pack crates (Option D) — until a domain has a heavy shared
  dependency.
- Any per-widget Cargo feature — until a concrete widget introduces a
  transitive dependency (Decision §4).

## Follow-up

This ADR discharges **both** open iteration-17 steering questions and
is the reference contract for the next slices:

1. **(Next slice, on its own — the mechanical move.)** Promote
   `widget::set_cell` to a public `Buffer` method in `rstui-core`;
   create `crates/rstui-widgets`; relocate `widget.rs`'s concrete
   widgets there (the `Widget` trait stays in core); repoint
   `text.rs`, the crate-root re-exports, every example/test, the
   README crate table, and `Cargo.lock`. The relocation is the *only*
   change in that iteration (no behavior change), proven by the
   widget/text test suites passing unchanged.
2. Subsequent widgets (List, Table, Tabs, …) land in `rstui-widgets`
   as new modules under the Decision §4 policy.
3. The umbrella `rstui` crate and any domain-pack / feature split are
   deferred until their concrete trigger (a second backend, or a
   widget with a heavy transitive dependency) exists.
