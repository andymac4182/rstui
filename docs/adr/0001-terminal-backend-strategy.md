# ADR 0001: Terminal backend strategy

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

rstui needs to put cells on a real terminal and read real input
eventually. Before that backend boundary is locked in, the strategy has
to be decided deliberately, because the choice is expensive to reverse
once a real driver and components depend on it.

Several constraints are **already locked in** by earlier slices and the
decision must fit them rather than relitigate them:

- `rstui-core` is dependency-free and pure. The `Backend` trait *and*
  the in-memory `TestBackend` already live in core
  (`crates/rstui-core/src/backend.rs`). The production terminal
  dependency must stay out of core.
- `Backend::draw` consumes the diff shape `IntoIterator<Item =
  (Position, &Cell)>` produced by `Buffer::diff`. The trait is
  intentionally non-object-safe and monomorphized over one concrete
  backend.
- rstui **owns its own input vocabulary** in core
  (`rstui_core::event::Event`/`KeyEvent`/`MouseEvent`), deliberately
  shaped 1:1 like the de-facto crossterm model, *specifically so that a
  real backend crate translates native events into rstui types*. This
  is a recorded, intentional divergence from ratatui (which re-exports
  crossterm's event types and does no input handling itself).
- The runtime's `Cmd` effects are already `Send + 'static`,
  anticipating a future threaded/async real runtime.
- The `Cell` is a single `char`; layout and the buffer are
  integer-only and immediate-mode. A retained native renderer is
  already off the table by prior design.

The open question, from the project steering note, is precisely: should
rstui use crossterm like ratatui, put crossterm behind a backend crate,
or explore other terminal/input backends or a renderer abstraction —
judged on portability, input fidelity, mouse/paste/focus, raw mode,
alternate screen, async integration, testability, and long-term
maintenance.

## Decision drivers

1. **Portability** — Linux, macOS, *and* Windows are table stakes for
   "build powerful terminal applications".
2. **Input fidelity** — keyboard (incl. Kitty protocol), mouse,
   bracketed paste, focus, resize, with a clean mapping into rstui's
   already-defined `Event`.
3. **Raw mode & alternate screen lifecycle** — must be enterable and,
   crucially, *restorable on panic*.
4. **Async integration** — a path to an event stream for the future
   async runtime without forcing async on the simple loop.
5. **Testability** — the deterministic `TestBackend`/`Harness` story
   must remain the default; the real backend must not become an
   untestable island.
6. **Maintenance & ecosystem** — minimize the surface rstui must
   maintain itself; maximize transfer of existing Rust TUI knowledge.
7. **Core purity** — `rstui-core` stays dependency-free.

## Options considered

### A. crossterm behind a dedicated backend crate (ratatui's model)

A new `rstui-crossterm` crate implements `rstui_core::Backend` and owns
the crossterm dependency, terminal lifecycle, and the crossterm →
`rstui_core::event::Event` translation.

- Cross-platform including Windows. Decodes keyboard (Kitty protocol in
  crossterm 0.29+), SGR mouse, bracketed paste, focus, resize. Provides
  raw mode and alternate screen. Optional tokio `EventStream` for
  async. Largest, most actively maintained Rust terminal crate; the
  default ratatui backend, so user knowledge transfers directly.
- Cost: one external dependency — fully isolated in the backend crate,
  core stays pure.

### B. termion behind a backend crate

- Unix-only (ratatui excludes it on Windows via target cfg). No focus
  events, no Kitty keyboard protocol, no async. Disqualifying for a
  cross-platform default; ratatui keeps it only as a niche Unix-purist
  option.

### C. termwiz behind a backend crate

- Cross-platform, best-in-class capability detection (wezterm
  pedigree), Kitty protocol, async, auto raw mode + alternate screen.
- But: heavier dependency tree, larger/more opinionated API, smaller
  ecosystem, only partial region-clear support. ratatui offers it only
  for "advanced terminal features", never as the default.

### D. Hand-rolled ANSI output + custom input parser (OpenTUI / Bubble
Tea model)

- Maximum control (sixel, OSC 8/52, synchronized output, pixel mouse).
- But: a very large, bug-prone, cross-platform surface — raw mode and
  console modes on Windows, an escape-sequence state machine, paste and
  Kitty edge cases. OpenTUI affords this only with a dedicated native
  Zig renderer team; Bubble Tea v2 does **not** hand-roll per project —
  it delegates parsing and rendering to the shared, maintained
  `charmbracelet/ultraviolet` + `x/ansi` + `x/term` libraries. The
  consistent lesson across references is *stand on a maintained
  terminal library*, not hand-roll one inside the framework.

## Decision

**Adopt Option A.** The recommended and default rstui terminal backend
is **crossterm**, implemented in a dedicated `rstui-crossterm` crate
that implements `rstui_core::backend::Backend`. `rstui-core` stays
dependency-free and keeps owning both the `Backend` trait and
`TestBackend`. The `Backend` trait is the single seam, designed so an
optional high-fidelity `rstui-termwiz` crate (and any future renderer)
remains implementable behind it later **without** changing core or
application code.

The `rstui-crossterm` crate specifically owns:

- The `Backend` impl over an `io::Write` (crossterm's queued ANSI/SGR):
  `draw` from the cell diff, cursor, clear, `size`, `flush`.
- A **terminal lifecycle RAII guard** that enables raw mode, the
  alternate screen, mouse capture, bracketed paste, and focus
  reporting, and **restores all of them on drop, including on panic**.
  This is a deliberate ergonomic improvement over ratatui (which leaves
  setup/teardown entirely to the application) and is possible because
  rstui's runtime owns the loop.
- A pure, TTY-free `from_crossterm(crossterm::event::Event) ->
  Option<rstui_core::event::Event>` translation. Because core's event
  vocabulary was deliberately shaped 1:1 like crossterm's, this map is
  near-mechanical and **unit-testable with hand-built events, no
  terminal required** — vindicating that earlier divergence.
- The input source: synchronous `poll`/`read` now; an async
  `EventStream` (crossterm `event-stream`, tokio) behind a crate
  feature for when the runtime goes async. The synchronous path never
  pulls in tokio.

termion is **rejected** outright (no Windows, no focus/Kitty). A
native renderer is **rejected** for rstui (it contradicts "idiomatic
Rust", adds a non-Cargo toolchain burden, and duplicates what crossterm
+ `Buffer::diff` already provide for an immediate-mode framework).
termwiz is **deferred, not rejected**: a clearly-scoped future optional
crate behind the same trait for users who need wezterm-grade fidelity.

### Trait-surface evolution

rstui's `Backend` deliberately lacks ratatui's `window_size` (pixel
size), `clear_region`, `append_lines`, and scroll-region methods.
Consistent with rstui's "defer, do not stub" discipline, these are
added **only when a concrete consumer needs them** (e.g. `window_size`
when an image/sixel widget lands; `append_lines`/scroll regions when a
scrollback or inline-print mode does), not speculatively. crossterm
supports all of them when the time comes, so this deferral costs
nothing now and keeps the trait small and reviewable.

## Evidence

Concrete facts gathered from the reference projects (read locally;
paths cited so the reasoning is auditable):

**ratatui** (`ratatui/main`) — the closest analog, and it validates
this exact shape at scale:

- `ratatui-core/src/backend.rs` holds the `Backend` trait *and*
  `TestBackend`; crossterm/termion/termwiz are **separate per-backend
  crates** (`ratatui-crossterm/`, `ratatui-termion/`,
  `ratatui-termwiz/`). rstui already mirrors the core split.
- `ratatui/Cargo.toml`: `default = ["crossterm", …]`; the comment
  states crossterm is "a reasonable choice for most applications as it
  is supported on Linux/Mac/Windows systems". `ratatui-termion` is
  gated `[target.'cfg(not(windows))']`.
- `ratatui/src/lib.rs`: "Ratatui does not include any input handling."
  Apps call `crossterm::event::read` directly; ratatui does **not**
  re-export event types. rstui's decision to own `Event` in core and
  translate in the backend crate is the recorded, intentional
  divergence — and is what makes the backend swap transparent to apps.
- Backend trait surface (`ratatui-core/src/backend.rs`): `draw`,
  hide/show cursor, get/set cursor position, `clear`,
  `clear_region(ClearType)`, `size`, `window_size`, `flush`,
  `append_lines`, feature-gated `scroll_region_{up,down}`. Confirms
  which methods rstui is deferring and that crossterm can back them.
- Capability matrix from the backend crates: focus events and Kitty
  protocol are present in crossterm and termwiz, **absent in termion**;
  async event streams in crossterm (`event-stream`) and termwiz, **not
  termion**.

**Bubble Tea v2** (`bubbletea/main`) — proves "don't hand-roll the
terminal layer per project":

- `tea.go`/`input.go` delegate *all* ANSI input parsing to
  `charmbracelet/ultraviolet`'s `TerminalReader`; `go.mod` pulls
  `x/term` (cross-platform raw mode), `x/ansi` (sequence constants),
  `ultraviolet` (parser + renderer). Even Charm extracted this into a
  shared maintained library rather than hand-rolling it in Bubble Tea.
- Platform split via build tags: `tty_unix.go` (termios) vs
  `tty_windows.go` (`ENABLE_VIRTUAL_TERMINAL_*`). This is exactly the
  cross-platform surface crossterm already encapsulates for Rust.
- Strongly-typed event messages (`KeyPressMsg`, `MouseClickMsg`,
  `PasteMsg`, `FocusMsg`, `WindowSizeMsg`) — the same modeling rstui's
  `Event` already uses.

**OpenTUI** (`opentui/main`) — shows the cost of the native path:

- Native Zig renderer (`packages/core/src/zig/renderer.zig`,
  ~87 KB) with its own cell-diff and a ~17-field `Capabilities` struct;
  a separate TypeScript escape-sequence state machine
  (`stdin-parser.ts`). No backend abstraction — hard-wired ANSI/Zig
  monolith; `testing: bool` only suppresses stdout (not a real mock).
  Justified only by a dedicated native team and retained-mode design —
  neither of which applies to rstui's idiomatic-Rust immediate-mode
  model.

**OpenCode** (`opencode/dev`) — a production app on OpenTUI, showing
how large the real backend compatibility surface is:

- `cli/cmd/tui/win32.ts` clears Windows `ENABLE_PROCESSED_INPUT` via
  FFI and re-asserts it on a 100 ms poll for a working Ctrl+C; Ctrl+Z
  is remapped on Windows; Windows Terminal <1.25 needs an empty-paste →
  image-clipboard fallback.
- `util/clipboard.ts` wraps OSC 52 in a tmux passthrough
  (`\x1bPtmux;…`) under SSH/`TMUX`.
- Conclusion: this compatibility surface is exactly what a maintained
  terminal library must absorb. Hand-rolling it inside rstui now would
  be a large, perpetual maintenance liability for no differentiating
  value; crossterm already carries most of it.

## End-to-end testing strategy (project requirement)

The steering note pairs the backend decision with making end-to-end
testing a first-class project requirement. This decision makes that
tractable and is therefore recorded here as an enforced contract: the
crossterm crate is the **only** non-deterministic component, so
everything else stays deterministically testable. rstui tests in four
layers, and every new public capability must land with the appropriate
layer:

- **L1 — Unit.** Pure `rstui-core`/`rstui-runtime` modules
  (geometry, style, layout, text, widget, cmd). Already in place.
- **L2 — App/component snapshot.** `TestBackend` render snapshots plus
  `Harness` event-sequence assertions over the real loop with no TTY.
  Required for every new widget and every app/runtime behavior.
- **L3 — Example smoke.** Every example is deterministic and doubles as
  a `TestBackend` snapshot assertion (pattern begun with `text_demo`).
  Now a project rule, not an ad-hoc nicety.
- **L4 — Backend integration** (unblocked by, and to land with, the
  `rstui-crossterm` crate): (a) pure unit tests of the
  `from_crossterm` event translation using hand-built crossterm events
  — no TTY needed; (b) a backend-output test that writes ANSI into an
  in-memory `io::Write` and asserts the emitted sequences for a known
  diff (feasible precisely because `Backend::draw` only needs a
  writer); (c) an opt-in, CI-only PTY smoke test of raw-mode setup and
  restore, behind `#[ignore]`/a feature so the default `cargo test`
  stays hermetic.

## Consequences

**Positive**

- Cross-platform from day one; the most-maintained Rust terminal crate
  carries the OS/console surface OpenCode proves is large.
- The backend swap is invisible to app code: apps depend only on
  `rstui-core`'s `Event` and the `Backend` trait, never on crossterm —
  a stronger isolation than ratatui's (whose apps import
  `crossterm::event`). termwiz can be added later as a pure addition.
- The RAII lifecycle guard means a panicking app still restores the
  user's terminal — a real ergonomic win over the ratatui baseline.
- The deterministic `TestBackend`/`Harness` story is untouched; the
  event translation is unit-testable without a TTY.

**Negative / accepted**

- One external dependency enters the workspace (isolated to
  `rstui-crossterm`; core stays pure). crossterm's API tracks the two
  most recent versions in the wider ecosystem — version churn is
  absorbed inside this one crate.
- Sixel/pixel-precision graphics and wezterm-grade capability detection
  are not available until an optional `rstui-termwiz` crate exists.
  Accepted: not needed for the core component set.

**Neutral / deferred**

- `window_size`, `clear_region`, `append_lines`, and scroll regions
  stay off the `Backend` trait until a concrete consumer needs them.
- The async `EventStream` path is feature-gated and unbuilt until the
  runtime goes async; the synchronous loop never depends on tokio.

## Follow-up

This ADR unblocks, and is the reference contract for, the next slices:

1. Create the `rstui-crossterm` crate: `Backend` impl over
   `io::Write`, the RAII terminal-lifecycle guard, and the pure
   `from_crossterm` translation with its TTY-free unit tests
   (testing layer L4 a/b).
2. Wire the real driver into `rstui-runtime` so the same `App` runs
   headless under `Harness` and live under crossterm unchanged.
3. Add the CI-only PTY smoke test (L4 c).

`rstui-termion` is rejected. `rstui-termwiz` is a possible future
optional crate behind the unchanged `Backend` trait, not scheduled.
