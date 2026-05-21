# Keyboard-shortcut review: rstui vs the field

A candid audit of how the three rstui apps (kitchen-sink, git-review,
acp-client) and the shared [`rstui-keymap`](keymaps.md) engine handle
keyboard shortcuts, benchmarked against the systems they were synthesised
from and the field at large: **opencode / opentui** (the SST agent TUI,
"pi"-class), **Textual**, **VS Code**, **Vim/Neovim + which-key.nvim**,
**Helix**, **Zellij**, **lazygit/gitui**, and the bare **ratatui**
baseline. It states where we lead, where we match, where we lagged, and
the concrete actions taken to close the real gaps.

> **Outcome:** the engine was already at or ahead of the field on the
> model; the review found exactly two genuine UX/quality gaps —
> **no which-key popup** and **no conflict audit** — both now closed
> (`Keymaps::continuations()`/`conflicts()` + the reusable `WhichKey`
> widget, adopted in the kitchen sink). Sequence depth and cross-app
> binding divergence were examined and are **deliberate**, documented
> below. Verdict: **best-in-class for a TUI keybinding system.**

---

## The axes that matter

A keyboard-shortcut system is judged on: (1) **semantic indirection** —
do shortcuts name *actions* or physical keys; (2) **discoverability** —
can a lost user find a binding without docs; (3) **customisation** — can
the end user remap without recompiling; (4) **context-sensitivity** — `q`
quits the shell but types in a field; (5) **sequences** — leader / multi-
key; (6) **cross-platform** — `⌘` vs `Ctrl`; (7) **the wiring cost** for
the app author; (8) **testability** — deterministic, headless. The
comparison is organised on these.

## Comparison matrix

| Axis | rstui | opencode/pi | Textual | VS Code | Vim+which-key | Helix | Zellij | ratatui |
|---|---|---|---|---|---|---|---|---|
| Semantic actions + stable id | ✅ `Action`+`id()` | ✅ action names | ✅ `Binding(id)` | ✅ command ids | ⚪ `:cmd`/funcs | ✅ command names | ✅ action enum | ❌ raw match |
| Reverse-lookup UI (footer/help) | ✅ `keys_for` live | ✅ | ✅ Footer | ⚪ cmd palette | ⚪ plugin | ✅ infobox | ✅ mode hints | ❌ |
| Which-key / leader popup | ✅ `WhichKey` (new) | ✅ | ❌ | ❌ | ✅ (plugin) | ✅ built-in | ⚪ mode bar | ❌ |
| User remap, no recompile | ✅ `RSTUI_KEYMAP` + in-app | ✅ JSON keybinds | ✅ `set_keymap` | ✅ keybindings.json | ✅ rc | ✅ TOML | ✅ KDL | ❌ |
| Merge-over-default + disable | ✅ id + `"none"` | ✅ `"none"` | ✅ | ✅ `-cmd` | ⚪ `unmap` | ⚪ replace | ⚪ replace | ❌ |
| Contexts / modes | ✅ `Capture{Layer,Text}` | ⚪ leader only | ✅ focus DOM | ✅ `when` expr | ✅ modes | ✅ modes | ✅ modes | ❌ |
| Leader + real timeout | ✅ ms, event-driven | ✅ | ❌ | ✅ chords | ✅ `timeoutlen` | ✅ | ⚪ | ❌ |
| Sequence depth | depth-2 (intentional) | 2 | 1 | 2 | ∞ | ∞ | 1 | — |
| Per-OS chord layer | ✅ compile-time `⌘/⌃` | ⚪ | ⚪ runtime | ❌ hardcoded | ⚪ | ❌ | ⚪ | ❌ |
| One-call wiring seam | ✅ `dispatch` | n/a (own loop) | ✅ framework | n/a | n/a | n/a | n/a | ❌ hand |
| Conflict audit | ✅ `conflicts()` (new) | ⚪ | ⚪ | ✅ editor | ✅ `:map` | ⚪ | ❌ | ❌ |
| Pure-data / headless-testable | ✅ (engine has no I/O) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ trivially |
| Idle cost of the system | ✅ 0 (no loop needed) | ⚪ | ⚪ | n/a | n/a | n/a | ⚪ | ✅ |

✅ first-class · ⚪ partial/indirect · ❌ absent

## Where rstui leads

- **Pure-data, headless-testable engine.** opencode/Textual/VS Code/Helix
  all bind keys to callbacks or live inside a runtime; rstui-keymap has
  *no* I/O, no clock of its own (the caller supplies `now_ms`), no
  callbacks — every path is a `Harness` unit test. Nothing else in the
  field guarantees this at the engine layer.
- **Per-OS at compile time, zero cost.** macOS gets `⌘`-native chords and
  `⌘⌥⌃⇧` display; Linux/Windows get `Ctrl/Super`. VS Code hardcodes
  `Ctrl`; Textual branches at runtime; we resolve it in `cfg!`.
- **Honest-ms leader, no animation loop.** The leader timeout is real
  milliseconds and resolution is event-driven with self-clearing — an
  idle rstui app costs 0% CPU, unlike systems that lean on a tick. (See
  [keymaps.md §"Do you need an animation loop?"](keymaps.md).)
- **One-call seam.** `dispatch(&ev,now)→{Act,Pending,Fall}` collapses the
  listen→map→act glue every other Rust TUI hand-writes; ratatui has
  nothing.
- **Contexts as pure data (ADR 0020).** The Vim-modes ⊕ VS Code-`when` ⊕
  Textual-focus synthesis, but additive and back-compat — an app sets one
  value on focus change instead of hand-ordering reducer guards.

## Where rstui matches the field

Semantic actions + stable ids (Textual), merge-over-default + `"none"`
disable + a serde-free config file + env override (opencode/VS Code),
multi-keymap by name/cycle (Vim/Zellij modes), the help/footer
reverse-lookup (Textual Footer / lazygit cheatsheet), and the universal
**help → `k`** discovery gateway (a deliberate rstui convention; see
[keymaps.md](keymaps.md)).

## Where rstui lagged — and what was done

1. **No which-key / leader popup.** opencode, Helix and which-key.nvim
   show a transient panel of *what can follow* an armed prefix; we had
   only a footer `armed()` hint and the full `KeymapView` editor.
   **Closed:** `Keymaps::continuations()` returns the armed prefix's
   `(key, action, help)`; the new engine-agnostic `WhichKey` widget
   renders the bottom-anchored popup; the kitchen sink shows it the
   instant `⟨leader⟩` (`Ctrl+X`) is pressed. The only app shipping a
   leader map; git-review/acp ship none and need nothing.
2. **No conflict detection.** A typo'd `RSTUI_KEYMAP` line or a
   copy-pasted binding could silently shadow an action (first-declared
   wins). Vim (`:map`) and VS Code surface this. **Closed:**
   `Keymaps::conflicts()` reports any trigger bound to >1 action; empty
   for every shipped map (asserted in tests), available to the editor.

## Examined and deliberately kept

- **Sequence depth = 2** (`Trigger::Key | Chain(Chord,Chord)`). Vim/Helix
  allow arbitrary depth (`g g`, space-menus). Decision: **keep depth-2.**
  Leader + one key + per-OS + contexts covers essentially every real TUI
  shortcut; arbitrary nesting is a large engine change (parse, the
  resolve state machine, display, which-key) for a long-tail idiom, and
  conflicts with the *cleanliness* the pure-data model buys. Documented
  as intentional in ADR 0015; reaffirmed here.
- **Cross-app binding divergence is correct, not sloppy.** Quit is
  `q`/`Esc` (kitchen-sink), `q`/`Esc`/`Ctrl+C` (git-review),
  `Ctrl+C`/`Ctrl+Q`/`F10` (acp-client); Help is `?` vs `F1`. This is
  **dictated by each app's input model**: acp-client's composer must let
  `q`/`?` be *typed*, so its globals are modifier/Fn chords; the
  browse-only apps can afford bare letters. Forcing uniformity would
  break the text apps. What *is* uniform — and is the thing that matters
  for discovery — is the **help → `k`** gateway into the keymap editor,
  identical in all three. The engine *enables* consistency
  (`RSTUI_KEYMAP`, the shared maps); per-app divergence here is a
  considered product choice.
- **acp-client uses no contexts (back-compat).** Its bespoke
  modal/permission/ask/completion cascade is flow-control, not
  "keybindings"; migrating it to nested contexts is the ADR 0020
  follow-up, not a defect.

## The per-app binding tables (as shipped)

**Engine — three built-in maps** (kitchen-sink uses these):

| Action | Default | Vim | Leader (⟨leader⟩ = `Ctrl+X`) |
|---|---|---|---|
| Quit | `q` `Esc` | `Esc` · `Z Z` | `Esc` · `⟨leader⟩ q` |
| Help | `?` | `g ?` | `?` · `⟨leader⟩ ?` |
| Palette | `:` `⌘K`/`⌃K` | `:` `/` | `⟨leader⟩ p` |
| Drawer (keymap mgr) | `g` | `g s` | `⟨leader⟩ s` |
| FocusToggle | `Tab` | `Ctrl+W` | `Tab` · `⟨leader⟩ w` |
| Goto 1–9 | `1`–`9` | `1`–`9` | `1`–`9` |
| Copy/Cut/Paste | `⌃/⌘ C/X/V` | `y/d/p` + `⌃/⌘` | `⟨leader⟩ y/d/v` + `⌃/⌘` |
| CycleKeymap | `F2` | `F2` | `F2` |
| ks.theme / ks.devtools | `Ctrl+T` / `F12` | (same) | (same) |
| *(in `text` Capture::Text context)* | only clipboard chords resolve; bare keys type | | |

**git-review** (own map; `input` = `Capture::Text` for filter/editor):
Quit `q`/`Esc`/`Ctrl+C` · Help `?` · Drawer `Ctrl+K` · Filter `/` ·
Focus `Tab` · Edit `e` · Split `s` · Orient `t` · Shrink `-` · Grow
`=`/`+` · Graph `\` · Theme `Ctrl+T`. Raw pane motions (`j/k`, `g/G`,
arrows, `[`/`]`, page) by design.

**acp-client** (global map; plugin-chord layer wins first): Quit
`Ctrl+C`/`Ctrl+Q`/`F10` · Help `F1` · Drawer `Ctrl+X`. Composer text
(full readline/emacs editing), modals, completion stay raw (bespoke
cascade) — the Drawer is on `Ctrl+X`, not `Ctrl+K`, precisely so it
cannot shadow the composer's readline `kill-line`.

All three: **help (`?`/`F1`) → `k`** opens the `KeymapView` editor — the
one universal, discoverable path; bindings remap via the editor or
`RSTUI_KEYMAP`.

## Recommendations / future (non-blocking)

- Surface `conflicts()` in the `KeymapView` footer when non-empty (a
  one-line "⚠ N ambiguous bindings").
- Optional: a `Keymaps::which_key_title()` so the popup shows the leader
  chord's display, not a fixed `⟨leader⟩`.
- If a future app genuinely needs space-menu depth, revisit depth-2
  behind a new ADR — not before a real need.

## See also

- [keymaps.md](keymaps.md) — the how-to (model, dispatch, contexts,
  which-key, conflicts).
- [ADR 0015](adr/0015-keymap-architecture.md) — the engine shape +
  Textual/opencode evidence.
- [ADR 0020](adr/0020-keymap-contexts.md) — contexts (Vim/`when`/Textual
  synthesis).
