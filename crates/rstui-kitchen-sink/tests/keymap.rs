//! Deterministic [`Harness`] coverage of the customisable keymap: cycling
//! keymaps, the opencode-style leader sequence, the live help/footer
//! re-deriving from the active map, and interactive re-binding + disabling
//! through the settings drawer. Same `KitchenSink` the binary runs.

use rstui_core::{Event, KeyCode, KeyEvent, KeyModifiers, Size};
use rstui_kitchen_sink::KitchenSink;
use rstui_runtime::Harness;

fn harness() -> Harness<KitchenSink> {
    Harness::new(KitchenSink::new(Size::new(120, 40)), 120, 40)
}
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}
fn key(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}
fn ctrl(c: char) -> Event {
    Event::from(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

#[test]
fn default_keymap_is_active_and_shown_per_os() {
    let h = harness();
    assert_eq!(h.app().active_keymap(), "Default");
    let s = h.snapshot();
    // The footer derives from the live keymap + detected OS.
    let os = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    };
    assert!(s.contains("Default"), "footer names the keymap:\n{s}");
    assert!(s.contains(os), "footer names the OS ({os}):\n{s}");
}

#[test]
fn f2_cycles_keymaps() {
    let mut h = harness();
    h.handle(key(KeyCode::F(2)));
    assert_eq!(h.app().active_keymap(), "Vim");
    assert!(h.snapshot().contains("Vim"));
    h.handle(key(KeyCode::F(2)));
    assert_eq!(h.app().active_keymap(), "Leader");
    h.handle(key(KeyCode::F(2)));
    assert_eq!(h.app().active_keymap(), "Default", "cycles back round");
}

#[test]
fn leader_sequence_opens_the_palette_in_the_leader_keymap() {
    let mut h = harness();
    h.handle(key(KeyCode::F(2))); // Vim
    h.handle(key(KeyCode::F(2))); // Leader (Ctrl+X prefix)
    // Plain `:` no longer opens the palette in this keymap.
    h.handle(ch(':'));
    assert!(
        !h.snapshot().contains("Go to screen"),
        "':' is not bound in the Leader keymap"
    );
    // Ctrl+X then `p` does.
    h.handle(ctrl('x'));
    assert!(h.is_running(), "leader armed, swallowed, no action yet");
    h.handle(ch('p'));
    assert!(
        h.snapshot().contains("Go to screen"),
        "⟨leader⟩ p opened the palette:\n{}",
        h.snapshot()
    );
}

#[test]
fn help_overlay_redrives_from_the_active_keymap() {
    let mut h = harness();
    h.handle(ch('?'));
    let s = h.snapshot();
    assert!(
        s.contains("Keyboard") && s.contains("Default"),
        "help titled with the active keymap:\n{s}"
    );
    assert!(s.contains("Command palette"), "help lists actions");
    // Switch keymap; the help overlay must follow (the Textual bug, fixed).
    h.handle(key(KeyCode::Esc)); // close help (Quit-confirm modal)
    h.handle(ch('n')); // decline quit
    h.handle(key(KeyCode::F(2))); // → Vim
    h.handle(ch('?'));
    assert!(
        h.snapshot().contains("Vim"),
        "help re-derived for the Vim keymap:\n{}",
        h.snapshot()
    );
}

#[test]
fn drawer_live_rebinds_an_action() {
    let mut h = harness();
    h.handle(ch('g')); // settings drawer
    let s = h.snapshot();
    assert!(
        // The shared `KeymapView` widget renders the action rows (the id
        // column clips in the 36-wide drawer, so assert the stable label).
        s.contains("Command palette") && s.contains("Settings drawer"),
        "drawer shows the keymap manager (KeymapView):\n{s}"
    );
    // The first row is the palette action; arm a rebind and capture `p`.
    h.handle(ch('r'));
    assert!(
        h.snapshot().contains("press a key to bind"),
        "rebind capture armed:\n{}",
        h.snapshot()
    );
    h.handle(ch('p')); // capture → palette now bound to `p`
    h.handle(key(KeyCode::Esc)); // close drawer
    h.handle(ch('p'));
    assert!(
        h.snapshot().contains("Go to screen"),
        "the remapped key opens the palette:\n{}",
        h.snapshot()
    );
    // Textual semantics: the old `:` is gone unless re-listed.
    h.handle(key(KeyCode::Esc)); // close palette
    h.handle(ch('n')); // (Esc opened quit-confirm) decline
    h.handle(ch(':'));
    assert!(
        !h.snapshot().contains("Go to screen"),
        "the old palette key was replaced by the remap"
    );
}

#[test]
fn drawer_disables_a_binding() {
    let mut h = harness();
    h.handle(ch('g')); // drawer
    h.handle(key(KeyCode::Down)); // row 1 = Help
    h.handle(ch('x')); // disable it
    assert!(
        h.snapshot().contains("Disabled"),
        "disabling is confirmed:\n{}",
        h.snapshot()
    );
    h.handle(key(KeyCode::Esc)); // close drawer
    h.handle(ch('?')); // would normally open the help overlay
    // The help overlay's title is unique (`Keyboard · <map> · <os>`); a
    // disabled binding must not open it. (The confirmation toast still
    // mentions the action name — assert on the overlay, not the word.)
    assert!(
        !h.snapshot().contains("Keyboard ·"),
        "the disabled binding no longer opens help:\n{}",
        h.snapshot()
    );
    assert!(h.is_running());
}

#[test]
fn remapping_does_not_break_existing_clipboard_or_nav() {
    // A regression guard: the keymap layer must be behaviour-identical to
    // the old hardcoded keys for the Default map.
    let mut h = harness();
    h.handle(ch('3')); // Navigation via digit
    assert!(h.snapshot().contains("fruit"), "digit jump still works");
    h.handle(key(KeyCode::Esc)); // quit-confirm
    assert!(h.snapshot().contains("Quit?"), "Esc still asks to quit");
    h.handle(ch('n'));
    assert!(h.is_running());
}

#[test]
fn help_then_k_is_the_universal_gateway_into_the_keymap_editor() {
    let mut h = harness();
    h.handle(ch('?')); // the universal "I'm lost" overlay
    let s = h.snapshot();
    assert!(
        s.contains("Customise these keybindings"),
        "help advertises the k gateway:\n{s}"
    );
    // `k` from help turns the cheat-sheet into the keymap editor.
    h.handle(ch('k'));
    let s = h.snapshot();
    assert!(
        s.contains("rebind") && s.contains("Settings drawer"),
        "help → k opened the KeymapView keymap editor:\n{s}"
    );
    assert!(h.is_running());
}
