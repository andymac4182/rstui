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
| B5 | OSC 8 hyperlinks (`ESC]8;;uri ST text ESC]8;; ST`) for the `Link` widget, **gated** on capability + tmux-off (fallback `text (url)`) | Rich/PI | prevents disappearing URLs |
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
