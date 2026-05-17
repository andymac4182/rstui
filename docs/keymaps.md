# Keymaps

`rstui-keymap` is the workspace's keybinding engine: **semantic actions
bound to key chords**, with multiple named keymaps, per-OS layers, leader
sequences, and live user remapping. It is the synthesis of how
[Textual][tx] and [OpenCode/opentui][oc] do it, adapted to rstui's
pure-reducer model (the *why* is [ADR 0015](adr/0015-keymap-architecture.md);
the canonical API is the crate's own rustdoc).

[tx]: https://textual.textualize.io/api/binding/
[oc]: https://opencode.ai/docs/keybinds/

It depends only on `rstui-core` (for `KeyCode`/`KeyEvent`/`KeyModifiers`),
owns no terminal or runtime state, and never panics on any input.

## The model

| Type | What it is |
|------|------------|
| `Action` | A semantic operation (`Palette`, `Copy`, `Goto(n)`, …) with a **stable string `id`** (Textual's binding id) for config/override lookup. Screen-level keys (arrows, typing) are deliberately *not* actions — they fall through to the focused screen raw. |
| `Chord` | A `KeyCode` + modifier set, **normalised** so `Ctrl+C`/`Ctrl+c`/`Shift`-implied uppercase compare equal. `parse("ctrl+shift+p")`, `from_event(&KeyEvent)`, a `parse`-able `spec()`, and an OS-aware `display()` (`⌘⌥⌃⇧` on macOS, `Ctrl/Alt/Super` elsewhere). |
| `Trigger` | A single `Chord`, **or** a two-chord sequence (opencode's `<leader> x`): a prefix chord then a key, within a timeout. |
| `Keymap` | A named set of `Action → Trigger`s + the leader chord/timeout. `keys_for(action)` is the **reverse lookup** the UI reads; `override_action(action, keys)` remaps (or `"none"` disables). |
| `Keymaps` | The registry: several keymaps, the active one, the user-override layer **merged over** it, and the leader state machine. `resolve(&KeyEvent, tick) -> Option<Action>` is the one call the reducer makes. |

The golden rule, same as Textual: **the shell reacts to actions, never to
physical keys**, and the UI is a *reverse lookup* of the live keymap so it
can never disagree with what a key actually does.

## Wiring it into an `App`

The keymap is caller-owned model state. The reducer resolves; the pure
view reverse-looks-up. This is the kitchen-sink pattern:

```rust
use rstui_keymap::{Action, Keymaps};

struct MyApp { keymaps: Keymaps, /* … */ }

// in `update`, for a key event:
match self.keymaps.resolve(&key_event, self.tick) {
    Some(Action::Palette)     => { /* open palette */ }
    Some(Action::Copy)        => { /* copy selection */ }
    Some(Action::CycleKeymap) => { let name = self.keymaps.cycle(); /* toast */ }
    Some(other)               => { /* … */ }
    None => {
        if self.keymaps.armed() {
            // a leader/prefix was pressed — swallow, wait for the rest
        } else {
            // unbound: hand the raw key to the focused screen
        }
    }
}

// on the animation tick, drop a leader that timed out:
self.keymaps.expire(self.tick);

// in `view`, the help/footer derive from the *live* map:
let km = self.keymaps.effective();
let palette_keys = km.keys_for(Action::Palette);   // e.g. ":" / "⌘K"
```

`resolve` returns `None` both when a key is unbound *and* when a leader
was just armed; `armed()` distinguishes them so an unbound key falls
through to the screen while a half-typed sequence is swallowed.

## Multiple keymaps

Three ship and `Keymaps::cycle()` rotates them (the kitchen sink binds
this to `F2`):

- **Default** — the app's normal bindings.
- **Vim** — Vim muscle memory, including leaderless sequences (`g?`,
  `Z Z`).
- **Leader** — opencode-style: `Ctrl+X` is a prefix with a 2 s timeout
  (`⟨leader⟩ p`).

`Keymaps::active_name()` and `status()` (name + whether a leader is armed)
feed the footer.

## Per-OS layers

Bindings are built at compile time with `cfg!(target_os = …)`: macOS gets
`⌘`-native chords, Linux/Windows get `Ctrl`/`Super`. The portable `Ctrl+*`
is **always also bound** (de-duped), so nothing is lost on a terminal that
never delivers `⌘`. `Chord::display()` renders `⌘⌥⌃⇧` on macOS and
`Ctrl+/Alt+/Super+` elsewhere; `Keymaps::os_name()` is the label.

## Customisation (remap & disable)

Exactly Textual's `set_keymap` / OpenCode's merged `keybinds`: an override
**replaces** an action's keys (the old key is gone unless re-listed) and
is **merged over** the defaults, so only what you change differs.

```rust
keymaps.set_override(Action::Palette, "ctrl+p"); // ':' no longer opens it
keymaps.set_override(Action::Help, "none");      // disabled; keys_for → "—"
```

In the kitchen sink the settings drawer (`g`) is a live keymap manager:
it shows the OS, the active keymap, the leader, and the full
`action → id → keys` table (built from `keys_for`), and lets you
**capture a key to rebind** an action or disable it — proving the
override path end to end. A config-file loader is intentionally *not* in
the crate (it stays serde-free); reading a `keymap.toml` and calling
`set_override` is the app's job.

## How the consumers use it

- **`rstui-kitchen-sink`** — every shell binding (quit, palette, help,
  drawer, focus, screen jump, copy/cut/paste, cycle keymap) is an
  `Action` resolved through `Keymaps`; the help overlay, footer hints and
  settings drawer are all reverse-lookups, so a keymap switch or a remap
  is reflected everywhere.
- **`rstui-acp-client`** — its plugin-keybind layer registers chords from
  plugin manifests and matches them against the live key. Both the
  registration side (`normalize_chord`) and the runtime side (`chord_of`)
  are thin adapters over `rstui_keymap::Chord`, so a plugin-declared
  `"ctrl+g"` and a pressed `Ctrl+G` canonicalise through the **same**
  parser — one vocabulary, by construction.

## Testing

Everything is deterministic and TTY-free. `Chord::parse`/`matches`/`spec`
are pure; `Keymaps::resolve` takes the tick as its clock so leader
timeouts are exact under the `Harness` (advance ticks, assert the
sequence). See `rstui-keymap`'s unit tests and the kitchen sink's
`tests/keymap.rs` (cycle, leader+timeout, help re-derivation, live
rebind, disable, per-OS) for the worked patterns.

## See also

- [ADR 0015](adr/0015-keymap-architecture.md) — why a shared crate and
  this shape.
- `rstui-keymap` crate rustdoc — the canonical API reference.
- [Architecture](architecture.md) · [Runtime](runtime.md) — the
  pure-reducer model the keymap fits into.
