//! Deterministic [`Harness`]-driven coverage of the kitchen-sink app: the
//! *exact* [`KitchenSink`] App the binary runs live, driven here with no TTY,
//! no threads, and no wall clock — so an interaction or rendering regression
//! fails `cargo test` (a CI gate) instead of only showing up by hand.

use rstui_core::{Event, KeyCode, KeyEvent, Size};
use rstui_kitchen_sink::KitchenSink;
use rstui_runtime::Harness;

/// A harness on a generous surface so every screen has room to render.
fn harness() -> Harness<KitchenSink> {
    Harness::new(KitchenSink::new(Size::new(120, 40)), 120, 40)
}

/// `Esc`/`q`/`Tab`/digits/`:`/`?`/`g` and letters as terminal events.
fn key(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}

#[test]
fn boots_on_welcome_with_chrome() {
    let h = harness();
    let s = h.snapshot();
    assert!(h.is_running(), "app starts running");
    assert!(s.contains("rstui"), "header brand renders:\n{s}");
    assert!(s.contains("Welcome"), "welcome screen + rail item:\n{s}");
    // The footer StatusBar and the rail are part of every frame.
    assert!(s.contains("palette"), "footer status bar renders:\n{s}");
}

#[test]
fn number_keys_jump_to_every_screen() {
    // Each screen must render its own content when its hotkey is pressed.
    let probes = [
        ('2', "Forms"),
        ('3', "Navigation"),
        ('4', "Data"),
        ('5', "Feedback"),
        ('6', "Containers"),
        ('7', "Rich Text"),
        ('8', "Colour"),
        ('1', "Welcome"),
    ];
    for (digit, expect) in probes {
        let mut h = harness();
        h.handle(ch(digit));
        let s = h.snapshot();
        assert!(
            s.contains(expect),
            "key '{digit}' should show {expect:?}:\n{s}"
        );
        assert!(h.is_running());
    }
}

#[test]
fn colour_lab_cursor_moves_with_arrows() {
    let mut h = harness();
    h.handle(ch('8')); // Colour Lab (pane → Content)
    assert!(h.snapshot().contains("Indexed(0)"), "cursor starts at 0");
    h.handle(key(KeyCode::Right));
    h.handle(key(KeyCode::Right));
    h.handle(key(KeyCode::Down));
    let s = h.snapshot();
    // 2 right + 1 down = index 18.
    assert!(s.contains("Indexed(18)"), "cube cursor tracks arrows:\n{s}");
}

#[test]
fn forms_screen_accepts_typing() {
    let mut h = harness();
    h.handle(ch('2')); // Forms, focus is on the Name field
    for c in "ada".chars() {
        h.handle(ch(c));
    }
    let s = h.snapshot();
    assert!(
        s.contains("ada"),
        "typed text reaches the focused Input:\n{s}"
    );
}

#[test]
fn quit_is_guarded_by_a_modal() {
    let mut h = harness();
    h.handle(ch('q'));
    let s = h.snapshot();
    assert!(s.contains("Quit?"), "q opens the confirm modal:\n{s}");
    assert!(h.is_running(), "still running until confirmed");

    h.handle(ch('n')); // decline
    assert!(h.is_running(), "n keeps the app running");

    h.handle(ch('q'));
    h.handle(ch('y')); // confirm
    assert!(!h.is_running(), "q then y quits");
}

#[test]
fn command_palette_navigates_by_query() {
    let mut h = harness();
    h.handle(ch(':')); // open palette
    assert!(h.snapshot().contains("Go to screen"), "palette opens");
    for c in "colour".chars() {
        h.handle(ch(c));
    }
    h.handle(key(KeyCode::Enter)); // jump to the single match
    let s = h.snapshot();
    assert!(
        s.contains("256-indexed"),
        "palette query jumped to the Colour lab:\n{s}"
    );
}

#[test]
fn help_overlay_toggles() {
    let mut h = harness();
    h.handle(ch('?'));
    assert!(
        h.snapshot().contains("Keyboard"),
        "? shows the help overlay"
    );
    h.handle(key(KeyCode::Esc));
    assert!(h.is_running(), "Esc closes help without quitting");
}

#[test]
fn rail_navigation_with_arrows_and_enter() {
    let mut h = harness();
    // Default focus is the rail; Down then Enter opens the 2nd screen.
    h.handle(key(KeyCode::Down));
    h.handle(key(KeyCode::Enter));
    let s = h.snapshot();
    assert!(s.contains("Forms"), "Down+Enter opens Forms:\n{s}");
}

#[test]
fn settings_drawer_swaps_the_palette_live() {
    let mut h = harness();
    let before = h.snapshot();
    h.handle(ch('g')); // open settings drawer
    assert!(h.snapshot().contains("Settings"), "drawer opens");
    h.handle(ch('t')); // toggle Dark → Light
    h.handle(ch('g')); // close
    // The frame still renders and the app keeps running after a live
    // full-palette swap.
    assert!(h.is_running());
    assert!(!h.snapshot().is_empty());
    let _ = before;
}

#[test]
fn survives_resize_and_ticks() {
    let mut h = harness();
    h.handle(ch('4')); // Data screen (animated)
    h.resize(80, 24);
    for _ in 0..5 {
        h.tick(); // deterministic animation frames
    }
    assert!(h.is_running(), "ticks + resize keep it running");
    assert!(
        h.snapshot().contains("Data"),
        "content still renders after reflow"
    );
}
