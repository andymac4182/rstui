# ADR 0015: Customisable keymap engine as a shared crate

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** rstui maintainers
- **Supersedes:** —

## Context

Two applications in the workspace need real, user-customisable
keybindings, and were each growing their own:

- `rstui-kitchen-sink` had a local `keymap` module: semantic actions,
  multiple keymaps (Default/Vim/Leader), per-OS chords, leader sequences,
  runtime overrides — built but private to a `publish = false` binary, so
  unreusable.
- `rstui-acp-client` had a *separate* bespoke pair — `chord_of`
  (`KeyEvent` → canonical string) and `normalize_chord` (plugin-declared
  string → canonical string) — to match plugin keybindings. Two
  hand-rolled chord vocabularies that had to agree by eye.

The forces already locked in, which this decision fits rather than
relitigates:

- **Immediate-mode, pure `view(&self)`; reducer is the sole mutation
  point** (ADR 0012, ADR 0004). A keymap is *caller-owned model state*:
  `update` resolves a key to an action and mutates; `view` only reads it
  back (for the help/footer). The framework never owns it.
- **`rstui-core` is dependency-free**; reusable, *generic* interaction
  primitives live in their own crate behind the core boundary (ADR 0002,
  the `rstui-widgets`/`rstui-runtime` shape).
- Totality (panic-free for any input) and the `cargo xtask ci` gates
  (fmt, lint-names, clippy `-D`, rustdoc `-D`, test) apply to every slice.

The expensive-to-reverse question: **where does the keybinding model
live, and what is its shape**, such that an app remaps keys per-user and
per-OS, ships several keymaps, supports leader sequences, and the help UI
always shows the *live* binding — without a retained tree, a callback, or
each app reinventing chord parsing.

## Decision drivers

1. **One model, many apps** — the kitchen sink and the ACP client (and
   future apps) must share *exactly one* chord vocabulary and resolution,
   not agree by coincidence.
2. **Caller-owned, immediate-mode honest** — no callbacks, no render-time
   mutation; `update` resolves, `view` reverse-looks-up.
3. **Customisable like the tools people know** — config-style remap and
   disable, merged over defaults.
4. **Per-OS and multi-keymap as first-class**, not bolted on.
5. **Dependency-light** — a keymap needs only the key vocabulary, so it
   may depend on `rstui-core` and nothing else.
6. **Totality & one documented pattern**, copyable by humans and agents.

## Options considered

### A. Leave it per-app (status quo)

Rejected. Two vocabularies that must agree by eye is the bug waiting to
happen; the kitchen sink's richer engine (leader sequences, overrides,
per-OS, reverse lookup) was unavailable to the ACP client at all.

### B. Put it in `rstui-core`

Rejected. `rstui-core` is the dependency-free substrate everything builds
on; a keymap is a *policy* layer, not substrate. ADR 0002's boundary
discipline says a self-contained concern that only needs the core
vocabulary gets its **own** crate (the `rstui-widgets`/`rstui-runtime`
precedent), keeping core small and slow-moving.

### C. A shared `rstui-keymap` crate (chosen)

A new library crate depending only on `rstui-core` for
`KeyCode`/`KeyEvent`/`KeyModifiers`. Both apps depend on it; the kitchen
sink's former module *is* the crate (history preserved by `git mv`); the
ACP client's `chord_of`/`normalize_chord` become thin adapters over the
crate's `Chord`, so plugin-declared and host-derived chords now go through
the **same** parser/normaliser.

## Decision

Ship **`rstui-keymap`** with this model — the synthesis of Textual and
OpenCode/opentui adapted to the pure-reducer architecture:

- **`Action`** — a semantic operation with a **stable string id**
  (Textual's binding id). The shell reacts to `Action::Palette`, never to
  "`:`". Screen-level keys (arrows, typing, `PageUp`) are deliberately
  *not* actions — they fall through to the focused screen raw, exactly as
  Textual bindings cascade past.
- **`Chord`** — a `KeyCode` + modifier set, **normalised** (letters fold
  to lowercase and drop `Shift`, since terminals deliver `Shift+a` as
  `A`), with `parse` (`"ctrl+shift+p"`, `"cmd+k"`, `"f2"`), `from_event`,
  a `parse`-able `spec`, and an OS-aware `display` (`⌘⌥⌃⇧` on macOS,
  `Ctrl/Alt/Super` elsewhere).
- **`Trigger`** — a single `Chord` *or* a two-chord **sequence**
  (opencode's `<leader> x`): a prefix chord then a key, with a per-keymap
  timeout. A leader chord can never also be a plain key (reserved).
- **`Keymap`** — a named set of binds (`Action` → `Trigger`s) plus the
  leader + timeout. `keys_for(action)` is the **reverse lookup** the help
  overlay / footer / settings read, so the UI always shows the *live*
  binding (fixing the Textual bug where the footer didn't follow a
  `set_keymap`). `override_action(id, keys)` replaces a binding, `"none"`
  disables it.
- **`Keymaps`** — the registry: several keymaps (Default, Vim, an
  opencode-style Leader), the active index, the user-override layer
  *merged over* the active map (opencode's `keybinds` merge / Textual's
  `set_keymap`), and the leader state machine driven on the deterministic
  tick clock so the headless `Harness` tests it.
- **Per-OS layering** at construction via `cfg!(target_os = …)`: macOS
  gets `⌘`-native chords; the portable `Ctrl+*` is always also bound
  (de-duped), so nothing is lost on a terminal that never delivers `⌘`.

`rstui-keymap` is the **canonical reference** (its crate-level rustdoc);
[`docs/keymaps.md`](../keymaps.md) is the how-to.

## Evidence

- **Textual** (`textual.binding`, *Custom keymaps in Textual*): a
  `Binding(key, action, description, id)`; a keymap is `{id → keys}`,
  comma-separated multi-keys, applied at runtime via `set_keymap`;
  original keys are *not* preserved unless re-listed. Known bug: footer /
  help did not follow a remap — `rstui-keymap` makes every UI surface a
  reverse lookup so it cannot drift.
- **OpenCode/opentui** (`opencode.ai/docs/keybinds`): action-name → comma
  separated combos; user `keybinds` **merged over built-in defaults**;
  `"none"`/`false` disables; a **leader** key (default `ctrl+x`,
  `leader_timeout` 2000 ms) for `<leader> x` sequences.

`rstui-keymap` takes Textual's stable-id override model and OpenCode's
merge + leader-sequence model, and adds first-class per-OS layering and a
reverse lookup that the immediate-mode UI consumes.

## Consequences

**Easy now**

- One keybinding model across the workspace; the ACP client's plugin
  keybinds and the kitchen sink's shell bindings share one chord
  vocabulary by construction, not by review.
- New apps get multi-keymap + per-OS + leader + remap by adding one
  dependency and resolving events through `Keymaps::resolve`.
- The help/footer/settings are reverse-lookups, so a keymap switch or a
  user remap is reflected everywhere with no extra wiring.

**Hard / deliberately deferred**

- No config-file *loading* yet (no serde in the crate): `override_action`
  takes the config-style string, but reading a `keymap.toml` is the
  consuming app's job (kept out so the crate stays dependency-light).
- Resolution is shell-level only by design — per-widget keymaps are not
  modelled; screens still own their raw keys (ADR 0004 routing).
- Chord vocabulary is the crate's (`bs`/`del` for Backspace/Delete);
  apps and plugins now normalise through it, so the spec is the contract.
