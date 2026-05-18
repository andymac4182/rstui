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
| `Keymaps` | The registry: several keymaps, the active one, the user-override layer **merged over** it, the leader state machine, and the **active context stack**. `dispatch(&KeyEvent, now_ms) -> Dispatch` (`Act`/`Pending`/`Fall`) is the one call the reducer makes; `now_ms` is a monotonic-ms clock, or just `0` when the map has no leader sequence. |
| `Context` + `Capture` | A binding may be scoped to a named context (`bind_in`); `Keymaps::set_context` activates one on focus/mode change. `Capture::Layer` (default) shadows globals but falls back to them; `Capture::Text` is a focused field — only its explicit binds resolve, every other key is raw (`Fall`). The Vim-mode / VS Code-`when` / Textual-focus model (ADR 0020). |

The golden rule, same as Textual: **the shell reacts to actions, never to
physical keys**, and the UI is a *reverse lookup* of the live keymap so it
can never disagree with what a key actually does.

## Wiring it into an `App`

The keymap is caller-owned model state. The reducer dispatches; the pure
view reverse-looks-up. One call — [`Keymaps::dispatch`] — is the whole
**listen → map → act** seam:

```rust
use rstui_keymap::{Action, Dispatch, Keymaps};

struct MyApp { keymaps: Keymaps, /* … */ }

// in `update`, for a key event. `0` is the clock — see below; an app
// whose keymap has no leader sequence passes `0` forever.
match self.keymaps.dispatch(&key_event, 0) {
    Dispatch::Act(Action::Palette)     => { /* open palette */ }
    Dispatch::Act(Action::CycleKeymap) => { let name = self.keymaps.cycle(); }
    Dispatch::Act(other)               => { /* … */ }
    Dispatch::Pending                  => { /* leader armed — swallow */ }
    Dispatch::Fall                     => { /* unbound — raw key → screen */ }
}

// in `view`, the help/footer derive from the *live* map:
let km = self.keymaps.effective();
let palette_keys = km.keys_for(Action::Palette);   // e.g. ":" / "⌘K"
```

That replaces the hand-written `resolve` + `armed()` + fall-through trio
every app used to copy-paste. (The kitchen sink keeps raw `resolve`
because it must *peek* the action — a clipboard chord fires even while an
overlay is up — but that is the rare advanced case; `dispatch` is the
norm.)

## Do you need an animation loop? **No.**

The `now_ms` argument is the *only* place time enters the keymap, and it
exists **solely** for the opencode-style leader-sequence timeout. The
rule, so apps stay at peak performance:

- **Resolution is event-driven.** Completing `⟨leader⟩ p` is decided by
  the *next key press*, never a clock — and a stale prefix **self-clears
  on the next key inside `resolve`**. So a leader sequence is fully
  correct with **no clock and no loop at all**.
- **If your keymap has no leader sequence** (the common case — only the
  opencode-style map ships one), the clock is dead weight: pass **`0`**
  forever. No timer, no tick, nothing.
- **`now_ms` is honest milliseconds** (`Instant::elapsed().as_millis()`
  live; a controlled value under the `Harness`) — *not* frames. There is
  no hidden "≈ a frame" assumption anymore, so you never need a render
  loop to make the unit meaningful.
- **`expire()` is optional and purely cosmetic** — it drops a stale
  *armed indicator* on a screen that is *truly idle* (a leader pressed,
  then nothing). Only an app that both ships a leader map *and* shows the
  armed hint needs it, and only from a clock **it already has** — never
  add a loop for it.

**Performance stance:** an idle TUI should cost ~0% CPU — render only
when state changes, never on a heartbeat (the `render()`-every-tick
idle-CPU anti-pattern, `RT-01` in [`docs/perf-review.md`](perf-review.md)).
The keymap is built so it *never* forces you off that path: the kitchen
sink runs an animation loop because it has spinners and a clock to
animate — it merely *reuses* that existing tick for the cosmetic
`expire`; git-review and acp-client run **no loop**, idle at zero cost,
and pass `dispatch(&ev, 0)`. Add an animation loop when you have
animation, not because of keymaps.

## Contexts: text inputs & modes (the easy way)

A key means different things in different places — `q` quits the shell
but must *type* into a focused field. **Do not** hand-order `if focused {
return … }` guards before `dispatch` (invisible, untyped, silently wrong
when broken). Scope the binding and flip one value on focus/mode change —
the model Vim (modes), VS Code (`when`) and Textual (per-widget focus)
all use, here as pure data (ADR 0020):

```rust
use rstui_keymap::{Action, Capture, Keymap, Keymaps};
const SAVE: Action = Action::Custom("app.save");

let mut keymaps = Keymaps::from_maps(vec![
    Keymap::new("app")
        .bound(Action::Quit, &["q"])              // global
        .bound_in("editor", SAVE, &["ctrl+s"])    // only while editing
        .bound_in("editor", Action::Quit, &["esc"]),
]);
keymaps.register_context("editor", Capture::Text);

// …in the reducer, exactly one call on focus/mode change:
keymaps.set_context("editor");   // entered the field
keymaps.set_context(None);       // left it
```

While `"editor"` is active, `dispatch(&ev, 0)` returns **`Fall` for `q`**
(so the widget types it) yet still **`Act(SAVE)` for `Ctrl+S`** — no
guard ordering, no `!ctrl && Char(_)` predicate. A context-scoped binding
*does not exist* outside its context, so `q` **cannot** fire Quit while
the editor is active even if the wiring forgot a guard — the footgun is
gone by construction.

- **`Capture::Text`** — a focused input / Vim-insert: only that
  context's explicit binds resolve, everything else is raw `Fall`.
- **`Capture::Layer`** (default) — a normal mode/pane (a "diff" view): its
  binds *shadow* globals, but an unbound key falls back to the global
  map. Register with `register_context(name, Capture::Layer)` or just
  don't register (Layer is the default).
- **Nesting** — `push_context`/`pop_context` for a modal over an editor
  over the shell; `set_context` is the flat single-mode shortcut.

The irreducible part: *something* must tell the keymap when focus/mode
changes (only the app knows what's focused). The win is **one typed
value vs fragile ordering**. An app that never sets a context is
byte-identical to before — contexts are purely additive.

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

### Which-key: discoverable leaders

A leader is only good if you can *find* what follows it.
`Keymaps::continuations()` returns `(key display, action, help)` for the
**currently-armed** prefix (empty when nothing is armed), and the
reusable
[`WhichKey`](widgets/overlays-and-control.md#whichkey) widget renders it
as the small bottom-anchored popup opencode / Helix / which-key.nvim are
known for. The reducer owns the armed state; the view does, every frame:

```rust
if self.keymaps.armed() {
    let rows = self.keymaps.continuations().iter()
        .map(|(k, _a, help)| (Cow::from(k.clone()), Line::from(*help)))
        .collect::<Vec<_>>();
    frame.render_widget(WhichKey::new(&rows), body); // bottom-anchored
}
```

The kitchen sink shows it the moment `⟨leader⟩` (`Ctrl+X`) is pressed;
`git-review`/`acp-client` ship no leader map so they never need it.

### Conflict audit

`Keymaps::conflicts()` reports any trigger bound to more than one action
in the active map (first-declared wins at resolve time, so the rest are
dead) — the audit Vim/VS Code surface and a typo'd `RSTUI_KEYMAP` or a
copy-paste can introduce. Empty for a well-formed map; cheap enough to
assert in a test or surface in the keymap editor.

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

### One universal way in: **help → `k`**

The hard part of "easy setting of keymaps" is *discovery* — a user who
wants to change a key looks at the **help overlay** first. So that is the
gateway, identically in every app: open help (`?` in the kitchen sink and
git-review, `F1` in acp-client), then press **`k`**. The help overlay
lists this line itself ("Customise these keybindings"), so it is
self-documenting; the always-visible footer/status of each app also
surfaces the keymap key. The direct shortcut still works for power users
(the kitchen sink's `Drawer` key — shown live in its footer; `Ctrl+K` in
git-review and acp-client). The rule: *if you can find help, you can find
— and change — every binding*, the same two keystrokes everywhere.

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
are pure; `dispatch`/`resolve` take the `now_ms` clock as a *caller*
argument (the engine never reads a real clock itself), so leader
timeouts are exact under the `Harness` — pass explicit millisecond
values and assert the sequence, no wall clock involved. See `rstui-keymap`'s unit tests and the kitchen sink's
`tests/keymap.rs` (cycle, leader+timeout, help re-derivation, live
rebind, disable, per-OS) for the worked patterns.

## See also

- [ADR 0015](adr/0015-keymap-architecture.md) — why a shared crate and
  this shape.
- [ADR 0020](adr/0020-keymap-contexts.md) — contexts (modes & text
  input): the Vim/`when`/Textual synthesis that amends 0015's
  shell-level-only stance.
- `rstui-keymap` crate rustdoc — the canonical API reference.
- [Architecture](architecture.md) · [Runtime](runtime.md) — the
  pure-reducer model the keymap fits into.
