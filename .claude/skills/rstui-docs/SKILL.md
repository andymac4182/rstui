---
name: rstui-docs
description: >-
  Keep the rstui documentation, component library, and VHS recordings in
  sync with the code. Use this whenever a widget is added/renamed/removed or
  its builder API changes; when rstui-core, the runtime, the plugin host, or
  the ACP client changes a public API; when a kitchen-sink screen changes;
  or when the user asks to update/regenerate the docs or component GIFs.
---

# Maintaining the rstui documentation

The docs are a **reference that must not drift from the code**. The source of
truth is always the code; this skill is the procedure for propagating a code
change into the docs, the component library, and the recordings, and proving
it stayed correct.

## The doc map (what documents what)

| Code that changed | Doc to update |
|-------------------|---------------|
| a `rstui-widgets` widget (new/renamed/removed/API) | `docs/widgets/README.md` (index) **and** the widget's family page |
| `rstui-core` public API (geometry/style/layout/buffer/terminal/event/focus/text/text_edit/text_area/scroll/selection/widget) | `docs/core-reference.md` |
| `rstui-runtime` / `rstui-crossterm` (`App`/`Cmd`/`Harness`/`run`/wiring) | `docs/runtime.md`, and `docs/testing.md` if the test surface changed |
| `rstui-plugin-host` (capability/manifest/policy/protocol/host/hooks/SDK) | `docs/plugins.md` |
| `rstui-acp-client` (modules, plugin layer, reference plugins) | `docs/acp-client.md` |
| a `rstui-kitchen-sink` screen / keybinding / chrome | `docs/kitchen-sink.md` and the e2e markers (below) |
| a new ADR / architectural boundary | `docs/architecture.md` ADR table + the relevant page |
| the VHS pipeline / tapes / `cargo xtask record` | `docs/recording.md` |

The widget family pages and their members:

- `docs/widgets/core-set.md` — Block, Paragraph, List, Tabs, Gauge,
  Scrollbar, Spinner, Table, Checkbox, Button, Radio, Input, Modal,
  StatusBar, Toast, Tree, Select, Editor
- `docs/widgets/rich-rendering.md` — Markdown, Link, Diff, Mermaid, Extmark,
  LineNumberGutter
- `docs/widgets/forms-and-data.md` — Slider, Switch, Form, MaskedInput,
  Sparkline, BarChart, Calendar, DatePicker, DescriptionList, Badge, Alert,
  Divider
- `docs/widgets/navigation-and-layout.md` — Menu, CommandPalette, Tooltip,
  Breadcrumb, SplitPane, Accordion, Card, Sidebar, Stepper, Pagination
- `docs/widgets/overlays-and-control.md` — ScrollView, Grid, Align, Popover,
  Drawer, Skeleton, Avatar, Kbd, HelpOverlay, Flow

The grouping mirrors the families in `crates/rstui-widgets/src/lib.rs` — if
the crate regroups, regroup the pages and update the README family table to
match.

## Where the truth lives

Read these before editing a widget entry — never paraphrase from memory:

1. `crates/rstui-widgets/src/lib.rs` — the structured rustdoc list of every
   widget (the authoritative one-liner + family).
2. `crates/rstui-widgets/src/<widget>.rs` — the module `//!` doc and the
   `impl` blocks (exact builder signatures, companion types, the state model
   — caller-owned vs pure layout vs owns-nothing).
3. `crates/rstui-widgets/examples/<name>_demo.rs` — confirms the demo command
   and what the GIF will show.

For core/runtime/plugins, the equivalent truth is that crate's `lib.rs` plus
the module under change.

## Playbook: a widget changed

1. Identify the family page from the doc map.
2. Read the widget module's `//!` and `impl` blocks. Update the entry to the
   exact current signatures and state model using the template in
   [`widget-entry-template.md`](widget-entry-template.md). Keep the entry
   shape identical to its neighbours (heading → GIF → purpose → companion
   types → state model → key API → demo).
3. If a widget was **added**: add the entry to its family page, add a row to
   *both* tables in `docs/widgets/README.md` (the family table and the
   alphabetical index, kept alphabetical), and confirm an
   `examples/<name>_demo.rs` exists (the GIF path is `media/<name>.gif`).
4. If a widget was **renamed/removed**: rename/remove the entry, both README
   rows, and the stale `docs/widgets/media/<old>.gif`.
5. Regenerate that widget's recording:
   `cargo xtask record widgets` (or just re-run its one tape). Commit the
   refreshed `docs/widgets/media/<name>.gif`.

## Playbook: a core / runtime / plugin / ACP API changed

Update the matching page from the doc map. Match the existing style: condensed
signatures in fenced `rust` blocks, the "total / pure projection / caller-owned"
framing where it applies, and an ADR link for any decision (`docs/adr/`). Do
not invent behaviour — quote the doc-comment intent.

## Playbook: the kitchen sink changed

Update `docs/kitchen-sink.md` (screens table, keybindings). If a header/footer/
screen literal that the e2e smoke asserts changed, update
`vhs/e2e/kitchen-sink-smoke.expect` to stable literals taken from
`crates/rstui-kitchen-sink/tests/kitchen_sink.rs` (those assertions are the
canonical, tick-counter-independent markers). Then regenerate the resolution
videos: `cargo xtask record kitchen-sink`.

## Conventions (non-negotiable, from ADR 0003 / 0012)

- **Example-first.** Every capability claim has a runnable command
  (`cargo run -p … --example …`) or a GIF.
- **ADR-linked.** Every architectural statement links the ADR that decided it.
- **Tables for reference data.** Signatures/grids go in tables or fenced
  blocks, not prose.
- **Hyphenated filenames** for docs (`core-reference.md`, not
  `core_reference.md`).
- **No banned name segments** anywhere a new `.rs`/module/pub item is added
  (`helper/util/common/misc/stuff/shared/thing*` — see
  `docs/conventions/naming.md`). Markdown/tape/data files are not scanned, but
  do not introduce them.
- **Pure projection vocabulary.** A widget *reads* caller-owned state; the
  reducer owns and mutates it. State every widget entry says so accurately.

## Verify before merging (the gate)

Markdown is not compiled, but the skill's deliverables still have hard checks:

```sh
cargo run -p xtask -- lint-names          # no banned names slipped in
cargo run -p xtask -- ci                  # full 5-gate fast loop (xtask compiles)
cargo xtask record e2e --check            # the real binary still renders the markers
# If a recording changed, regenerate and eyeball it:
cargo xtask record widgets|kitchen-sink|gallery
```

`cargo xtask record` needs the VHS toolchain (`brew install vhs ttyd ffmpeg`)
and `VHS_NO_SANDBOX=true` (use `scripts/record-demos.sh`, which sets it). It is
**not** a CI gate by design; the e2e `--check` is the regression assertion you
run by hand when touching the kitchen sink.

## Landing the change

Follow the canonical merge protocol in `docs/merging.md`: commit the doc/media
slice, take the serialized lock, fetch + rebase `origin/main`, re-validate the
*merged* main with `cargo xtask ci`, push, release the lock, rebase. Do a
merge-back per coherent slice — never batch docs to the end of a session.

## Self-check: is a doc stale?

A page is stale if any of these is true — fix it the same way the code change
would have:

- a widget exists in `crates/rstui-widgets/src/` with no entry on its family
  page, or no row in both `docs/widgets/README.md` tables;
- an entry's builder signatures or state-model line no longer match the
  module's `impl`/`//!`;
- `docs/widgets/media/<name>.gif` is missing for an existing widget;
- a `docs/*.md` API block contradicts that crate's current `lib.rs`;
- the e2e markers are not present in a fresh `cargo xtask record e2e --check`.
