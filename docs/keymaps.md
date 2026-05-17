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

Three ship. `Keymaps::cycle()` rotates them (the kitchen sink binds this
to `F2`); `Keymaps::set_active("Vim")` jumps **straight to one by name**
(case-insensitive, unknown name is a no-op), and `Keymaps::map_names()`
lists the choices for a UI or config doc:

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
override path end to end.

## End user: a config file (no app UI, no rebuild)

The user doesn't *need* an in-app rebind UI. `Keymaps::load_overrides`
parses a trivial text file — one `id = keys` per line, full-line `#`
comments and blanks ignored, unknown ids skipped (a typo never breaks the
other keys), `keymap = Name` picks the active map:

```text
# ~/.config/myapp/keymap — edit and restart
keymap      = Vim          # pick the active map by name
app.palette = ctrl+p, /    # remap (comma-separates alternatives)
app.help    = none         # disable an action
myapp.save  = ctrl+k       # app-defined actions too (see below)
```

`keys` are exactly [`Chord::parse`](#the-model) tokens; `id`s are the
stable `Action::id()` strings (`Keymaps::action_for_id` is the inverse and
also resolves app actions). It is **serde-free by design** — hand-parsed,
same ethos as `Chord::parse` (ADR 0002), no dependency added. The crate
takes the text; reading the file is the app's one-liner.

The kitchen sink wires this through **`RSTUI_KEYMAP`**, exactly mirroring
`RSTUI_THEME`: set it to a built-in map name *or* a path to a keymap
file — no rebuild, no UI:

```sh
RSTUI_KEYMAP=Vim                 cargo run -p rstui-kitchen-sink
RSTUI_KEYMAP=~/.config/rstui/keymap  cargo run -p rstui-kitchen-sink
```

An unknown name or unreadable/invalid file keeps the defaults.

## App-defined actions

The built-in `Action`s are a starter set, not a closed one. An app adds
its **own** actions with `Action::Custom("myapp.save")` and registers
them *on top of* every shipped map with `Keymaps::bind` — one call, and
they behave exactly like a built-in (resolve, help/footer reverse-lookup,
user override, config file all just work):

```rust
const SAVE: Action = Action::Custom("myapp.save");

let mut keymaps = Keymaps::new();   // keeps Quit/Help/Palette/…
keymaps.bind(SAVE, "ctrl+s");       // …and adds yours, in every map
// resolve → Some(SAVE); keys_for(SAVE) lights the footer; the
// `myapp.save = …` config line above remaps it — no extra code.
```

An app whose vocabulary is entirely its own builds complete maps with
`Keymap::new("MyApp").bound(SAVE, &["ctrl+s"])` and
`Keymaps::from_maps(vec![…])` instead of the batteries-included three.

## UI: the `KeymapView` widget

The visual half of "easy setting of keymaps" is a reusable widget, not
per-app chrome: [`KeymapView`](widgets/overlays-and-control.md#keymapview)
(in `rstui-widgets`) renders the live keymap as a selectable table with a
per-row state (selected / **capturing** / disabled), an id column, scroll
windowing and `hit()` for click-to-rebind. It is **engine-agnostic** — it
takes plain `KeymapRow`s, so `rstui-widgets` keeps its `rstui-core`-only
boundary (ADR 0002) and never depends on `rstui-keymap`. An app adapts its
registry into rows in `view`:

```rust
let km = self.keymaps.effective();
let rows: Vec<KeymapRow> = self.actions.iter().map(|&a| {
    let keys: Vec<String> = split_caps(&km.keys_for(a)); // OS-aware caps
    KeymapRow::new(a.help(), keys).id(a.id()).state(
        if Some(a) == self.rebind { RowState::Capturing }
        else if a == self.actions[self.sel] { RowState::Selected }
        else if km.keys_for(a) == "—" { RowState::Disabled }
        else { RowState::Normal })
}).collect();
KeymapView::new(&rows).header(/* map · OS · leader */);
```

The reducer still owns the cursor and the capture FSM (arm on a key,
`Chord::from_event(&ev).spec()` → `set_override`, `Esc` cancels); the
widget only draws state and reports the clicked row. All three apps
(kitchen sink, acp-client, git-review) use this one widget.

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
