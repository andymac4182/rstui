# 0013 — Terminal-emulator compatibility & control-code posture

- Status: Accepted
- Date: 2026-05-17
- Deciders: rstui maintainers
- Supersedes/relates: [0001](0001-terminal-backend-strategy.md) (crossterm
  backend), [0012](0012-widget-composition-and-layout-model.md) (totality)

## Context

rstui emits ANSI/escape sequences through `rstui-crossterm` (the only crate
allowed an external dependency, ADR 0001). A control-code that is wrong, mis-
ordered, or unconditionally assumed breaks differently on every emulator —
the single hardest TUI class to test and the easiest to regress.

A component-by-component and control-code deep dive was done against four
mature terminal UIs:

- **Textual** (`Textualize/textual` + Rich) — env-driven colour-system
  detection, `?2026`/`?2048` DECRPM queries, strict reverse-order signal/
  panic-safe teardown, Apple-Terminal quirk skip.
- **opentui** (`sst/opentui`) — lazy per-frame synchronized output, aggressive
  runtime capability probe (XTVERSION/DA1/batched DECRPM), the broadest
  per-emulator quirk table, Zig colour-degrade path.
- **OpenCode** (`sst/opencode`, Go bubbletea history + the `@opentui` rewrite)
  — vendored input parser hardening (URxvt/X10/WezTerm), OSC 52 + tmux
  passthrough, OSC 11 background query with a WSL guard, Win32 console-mode
  FFI workarounds.
- **PI** (`earendil-works/pi`, `@earendil-works/pi-tui`) — a hand-written
  differential renderer: `?2026` on every frame (incl. partial diffs), a
  partial-chunk-safe stdin reassembler with idle-flush, Kitty-keyboard query/
  enable/teardown + `modifyOtherKeys` tmux fallback, input-drain on exit,
  WezTerm `\x1b\x1b[` Esc guard, emoji/flag width-2 streaming-drift fix,
  capability-gated OSC 8 / images, inline Image (Kitty graphics / iTerm2
  OSC 1337).

Components: rstui's ~57-widget set already exceeds both app-specific TUIs
(OpenCode, PI). The only genuinely missing primitives are an inline **Image**
widget (Kitty graphics + iTerm2 OSC 1337) and a **QR** widget — both deferred
(see backlog), neither blocking.

## Decision

**Never assume terminal capabilities; emit only universally-safe control
codes unconditionally, and degrade everything else from a detected ceiling.**

Concretely, landed now:

1. **Synchronized output.** Every non-empty frame's `draw()` is bracketed in
   `ESC[?2026h` … `ESC[?2026l`, lazily (only on the first changed cell, so an
   idle frame still emits zero bytes). Unknown-mode-safe on terminals without
   2026. This is the single highest-impact anti-flicker fix
   (`backend.rs::draw`).
2. **Colour by capability, never blind truecolor.** `rstui-core` gained a
   pure, total `Color::degrade`/`to_indexed`/`to_ansi16` and a `ColorLevel`
   using the Rich/xterm algorithm (saturation-gated 232–255 grey ramp, non-
   linear 6×6×6 cube, redmean nearest-16). `rstui-crossterm` detects the
   level from a conservative env ladder — `FORCE_COLOR` → `NO_COLOR` →
   not-a-tty → `COLORTERM` ∈ {truecolor,24bit} → `TERM` (`*-256color` ⇒ 256,
   `dumb` ⇒ none, else the 16-colour floor) → 256 default — and `run_app`
   applies it. A terminal must *positively* advertise truecolor.
3. **Scrollback purge on full clear.** `clear()` emits `ESC[2J` + home +
   `ESC[3J` so a full redraw leaves no stale history above the frame.

Teardown was audited and confirmed correct: `lifecycle.rs` single-sources a
reverse-order, panic-/signal-safe restore (raw off → Kitty pop → focus off →
paste off → mouse off → alt-screen off), byte-identical between the on-`Drop`
guard, the panic hook, and the signal thread.

Design rules going forward:

- Universally-ignored-if-unsupported private modes (`?2026`, `?2004`,
  `?1004`, Kitty push/pop) may be emitted unconditionally.
- Anything a terminal can *misrender* (truecolor SGR, OSC 8, graphics) MUST
  be gated on positive detection (env ladder now; DECRPM/XTVERSION upgrade
  later).
- All quantisation/parsing logic stays **pure and total** in `rstui-core`
  (testable with no TTY, no panic on any input — the iter-25 rule); only the
  thin env/IO detection lives in `rstui-crossterm`.
- Restore is exact-reverse, idempotent, and signal/panic-safe — non-
  negotiable.

## Prioritised backlog (deep-dive findings not yet landed)

Recorded so the research is actionable, not lost. Roughly priority order:

| # | Item | Source | Note |
|---|------|--------|------|
| B1 | Kitty keyboard query (`ESC[?u`) + progressive enable + clean pop | PI/Textual | rstui pushes flags unconditionally today (safe, ignored by non-Kitty); a query lets it *know* and avoids redundant pop |
| B2 | `modifyOtherKeys` (`ESC[>4;2m`/`;0m`) fallback when Kitty absent (tmux) | PI | materially better modified-key fidelity under tmux/old xterm |
| B3 | tmux/screen **DCS passthrough** wrapping for any outgoing capability query / OSC 52 (`ESC P tmux; … ESC \`, ESC-doubled) | opentui/OpenCode | required or the multiplexer eats queries / clipboard |
| B4 | Partial-chunk-safe stdin escape reassembler with idle-flush + WezTerm `\x1b\x1b[` Esc guard + X10-mouse incomplete guard | PI/OpenCode | only if rstui ever parses raw stdin itself (currently delegated to crossterm — verify it does this) |
| B5 | OSC 8 hyperlinks — see the concrete cross-crate design in [Link activation](#link-activation-rstui-vs-textual) below; **gated** on capability + tmux-off (fallback `text (url)`) | Rich/PI | terminal-native click/copy; prevents disappearing URLs |
| B6 | OSC 52 clipboard (`ESC]52;c;b64 BEL`) + tmux passthrough + native-CLI fallback | OpenCode | copy-over-SSH |
| B7 | OSC 11 background query for true light/dark theme adaptation, **WSL-guarded** + timeout (HSL-lightness < 0.5 heuristic) | OpenCode | known hang on WSL |
| B8 | Leave-sequence hardening: also emit `ESC[0m` + show cursor + `ESC[0 q` (default cursor shape) at teardown start | opentui | defensive vs. panic mid-frame |
| B9 | Cursor-shape API (DECSCUSR `ESC[n q`) + restore `ESC[0 q` | — | text-input UX |
| B10 | SIGWINCH-on-start (Unix) for fresh size after `fg`; SIGCONT re-query | PI | needs unsafe-free signal raise (signal-hook can't send) — design needed |
| B11 | Emoji/flag/ZWJ display-width (treat singleton regional indicator as width 2 to avoid streaming drift); cheap pre-filter before regex | PI | rstui buffer width is 1 cell/char today — a broader renderer concern (ADR 0012 deferral) |
| B12 | Per-emulator quirk struct from `TERM_PROGRAM`/`WT_SESSION`/`TMUX`/XTVERSION; Apple-Terminal skip `?2026` probe; Windows VT-input/`ENABLE_VIRTUAL_TERMINAL_PROCESSING`, build ≥ 15063 truecolor gate | opentui/Textual | drives B1/B3/B5 decisions |
| C1 | Inline **Image** widget (Kitty graphics + iTerm2 OSC 1337, chunked, id-reuse, tmux-disabled) | PI | the one notable component gap |
| C2 | **QR** widget (half-block cells) | OpenCode | minor component gap |

B11/C1/C2 are renderer/component scope (ADR 0002/0012), not control-code; the
rest are `rstui-crossterm`-local.

## Consequences

- A truecolor theme now renders correctly on 256/16/no-colour terminals and
  over SSH instead of emitting ignored `38;2` escapes; flicker on slow links
  is eliminated where `?2026` is supported; full redraws no longer strand
  scrollback. All three are unit-tested with no TTY (in-memory `Vec<u8>` byte
  assertions + the pure colour tests in `rstui-core`).
- The conservative default (256, never assume truecolor) means a modern
  truecolor terminal that does not set `COLORTERM=truecolor` is rendered at
  256-colour. Accepted: correctness over optimism; most modern terminals set
  it, and `FORCE_COLOR=3` / a future DECRPM upgrade override it.
- The backlog is explicit and sourced, so the remaining hardening is a
  tracked queue, not rediscovery. Each item is independently shippable
  through `rstui-crossterm` with no `rstui-core` dependency change.

## Regression lock

The per-fix unit tests were scattered (`backend.rs`/`style.rs`), so a
refactor could silently break one. The single discoverable, end-to-end
guard is **`crates/rstui-crossterm/tests/control_code_contract.rs`** — it
asserts every byte-level property here as one contract (sync envelope incl.
lazy-open ordering; colour degraded through `draw` *and* the production
`run` loop for every level/kind incl. `Indexed(>=16)@Ansi16`; scrollback
`ESC[3J`), so any regression fails `cargo xtask ci` *by name*. Writing it
also surfaced and fixed a real defect: `draw`'s running-state minimisation
now tracks the **degraded** colour, so a colour that degrades to default no
longer emits a redundant `SetColors` (at `NoColor`, zero colour escapes).
New ADR-0013 control-code work must extend that contract test.

## Link activation: rstui vs Textual

How Textual makes a link clickable, and rstui's immediate-mode equivalent.

**Textual has two distinct mechanisms.** (1) *Terminal-native* OSC 8: Rich
renders `Style(link=URI)` as `ESC]8;id={id};{URI}ESC\ {text} ESC]8;;ESC\`
— an auto `id` groups a multi-cell/wrapped span so the *terminal* underlines
and Cmd/Ctrl-click-opens it; the empty `ESC]8;;ESC\` closes the run.
(2) *In-app* `[@click=action]` markup: the action string is attached to each
character span's `Style.meta`; on `MouseDown` Textual walks its **retained
DOM** (`get_widget_at`), reads the per-cell `style.meta` at the click offset
from the widget's `Strip`, parses the action and dispatches via
`run_action`. A markdown link bubbles a `Markdown.LinkClicked(href)`
message — Textual itself does **not** open a browser; the app handler does.

**rstui is immediate-mode: no retained DOM, no per-cell meta map.** The
equivalent of mechanism (2) is the reducer hit-testing the click against the
same area it rendered into. That path now exists and is *locked*:
`Markdown::link_activation_at(pos, area) -> Option<LinkActivation>` (and the
same on `Mermaid`) resolves a click to `{index, href}` from a *single* parse
of the immutable source — eliminating the `link_at` + `links()[i]`
index-desync foot-gun the old two-call pattern invited. The full
`Event::Mouse → link_activation_at → href` pipeline is regression-locked by
`crates/rstui-smoke/tests/link_click_e2e.rs` (a real `App` under `Harness`).
rstui, like Textual, surfaces the activation to the app (`LinkActivation`),
never opening a URL itself.

**What is still missing is mechanism (1)** — terminal-native OSC 8 (so a
plain mouse click / copy works without app hit-testing). Concrete design
(this is backlog B5, now specified, not vague):

- `Cell` gains `link: Option<NonZeroU16>` — a per-frame interned hyperlink
  id, 2 bytes, keeps `Cell` `Copy` and small (no `String` in the hot
  buffer). `Buffer` holds the `id → href` table (a `Vec<Box<str>>`); a new
  `Buffer::set_hyperlink(href) -> id` + the widget stamps cells with it
  (`Markdown`/`Mermaid`/`Link` already know their hrefs + regions).
- `Backend::draw` is given the table (extend the diff item, or pass the
  buffer's link table alongside): when the running link id changes it emits
  `ESC]8;id={id};{href}ESC\`, and `ESC]8;;ESC\` when it returns to `None`.
  Closing on a *frame* boundary too, so an unclosed run never leaks.
- **Capability-gated** exactly like colour (ADR 0013 §Decision): only when
  the terminal advertises support; **disabled under tmux/screen** (they
  swallow OSC 8 → invisible URLs), where the widget falls back to rendering
  `text (url)`. Add the assertion to the control-code contract test.

This is a multi-crate change to the hot `Cell`/`Buffer`/`Backend` path; it
is deliberately deferred to its own slice rather than rushed, but the design
is now fixed so it is mechanical to land.
