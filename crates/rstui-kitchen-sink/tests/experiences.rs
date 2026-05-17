//! Deterministic [`Harness`] coverage of the ten "real app" experience
//! screens — the same [`KitchenSink`] App the binary runs, driven with no
//! TTY so an interaction regression fails `cargo test`.

use rstui_core::{Event, KeyCode, KeyEvent, Size};
use rstui_kitchen_sink::KitchenSink;
use rstui_runtime::Harness;

fn harness() -> Harness<KitchenSink> {
    Harness::new(KitchenSink::new(Size::new(120, 40)), 120, 40)
}
fn key(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}
fn typed(h: &mut Harness<KitchenSink>, s: &str) {
    for c in s.chars() {
        h.handle(ch(c));
    }
}
/// Open a screen by fuzzy command-palette query.
fn goto(h: &mut Harness<KitchenSink>, query: &str) {
    h.handle(ch(':'));
    typed(h, query);
    h.handle(key(KeyCode::Enter));
}

#[test]
fn rail_is_grouped_into_widgets_and_experiences() {
    let s = harness().snapshot();
    assert!(s.contains("WIDGETS"), "rail has a Widgets section:\n{s}");
    assert!(s.contains("EXPERIENCES"), "rail has an Experiences section");
    assert!(s.contains("Chat"), "Chat appears in the rail");
    assert!(s.contains("Kanban"), "Kanban appears in the rail");
}

#[test]
fn nine_jumps_to_chat() {
    let mut h = harness();
    h.handle(ch('9'));
    let s = h.snapshot();
    assert!(s.contains("Channels"), "Chat rail renders:\n{s}");
    assert!(s.contains("#general"), "seeded channel shows");
}

#[test]
fn chat_send_appends_message_and_canned_reply() {
    let mut h = harness();
    h.handle(ch('9')); // Chat, content focused
    typed(&mut h, "hello"); // chars reach the composer (text-entry screen)
    h.handle(key(KeyCode::Enter)); // send
    let s = h.snapshot();
    assert!(
        s.contains("hello"),
        "the sent message is in the thread:\n{s}"
    );
    assert!(
        s.contains("Noted:"),
        "the peer canned-replied (thread stays live):\n{s}"
    );
    assert!(h.is_running());
}

#[test]
fn chat_switches_channel_with_arrows() {
    let mut h = harness();
    h.handle(ch('9'));
    h.handle(key(KeyCode::Down)); // next channel
    let s = h.snapshot();
    assert!(s.contains("#rust"), "second channel is reachable:\n{s}");
}

#[test]
fn command_palette_reaches_every_experience() {
    for (query, expect) in [
        ("mail", "Folders"),
        ("files", "Explorer"),
        ("dashboard", "Revenue"),
        ("music", "Playlist"),
        ("code editor", "Problems"),
        ("settings", "Settings"),
        ("kanban", "Backlog"),
        ("live logs", "filter:"),
    ] {
        let mut h = harness();
        goto(&mut h, query);
        let s = h.snapshot();
        assert!(
            s.contains(expect),
            "palette {query:?} should open a screen showing {expect:?}:\n{s}"
        );
        assert!(h.is_running());
    }
}

#[test]
fn board_moves_a_card_across_columns() {
    let mut h = harness();
    goto(&mut h, "kanban");
    assert!(h.snapshot().contains("Backlog"), "board renders");
    h.handle(key(KeyCode::Right)); // move selected card right
    let s = h.snapshot();
    assert!(s.contains("Moved to"), "moving a card raises a toast:\n{s}");
}

#[test]
fn login_validates_then_succeeds() {
    // Empty submit → error.
    let mut h = harness();
    goto(&mut h, "sign in");
    h.handle(key(KeyCode::Down)); // focus password
    h.handle(key(KeyCode::Enter)); // submit empty
    assert!(
        h.snapshot().contains("Sign-in failed"),
        "empty credentials are rejected"
    );

    // Correct credentials → success.
    let mut h = harness();
    goto(&mut h, "sign in");
    typed(&mut h, "ada"); // username
    h.handle(key(KeyCode::Down)); // → password
    typed(&mut h, "rust");
    h.handle(key(KeyCode::Enter)); // submit
    let s = h.snapshot();
    assert!(
        s.contains("Welcome back"),
        "valid credentials sign in:\n{s}"
    );
}

#[test]
fn logs_stream_grows_with_ticks_and_filters() {
    let mut h = harness();
    goto(&mut h, "live logs");
    assert!(h.snapshot().contains("filter:"), "log viewer renders");
    for _ in 0..12 {
        h.tick(); // the synthetic stream grows deterministically
    }
    let s = h.snapshot();
    assert!(
        s.contains("INFO") || s.contains("ERROR") || s.contains("WARN"),
        "lines stream in over ticks:\n{s}"
    );
    typed(&mut h, "error"); // live substring filter (text-entry screen)
    assert!(h.is_running());
}

#[test]
fn ide_is_editable_and_tracks_the_cursor() {
    let mut h = harness();
    goto(&mut h, "code editor");
    let before = h.snapshot();
    assert!(before.contains("Problems"), "editor + problems pane render");
    assert!(
        before.contains("Ln 1, Col 1"),
        "status bar shows the cursor"
    );
    typed(&mut h, "Xy");
    let s = h.snapshot();
    assert!(s.contains("Ln 1, Col 3"), "typing advances the caret:\n{s}");
}

#[test]
fn experiences_survive_resize_and_ticks() {
    let mut h = harness();
    for q in ["dashboard", "music", "files", "mail"] {
        goto(&mut h, q);
        h.resize(90, 28);
        h.tick();
        h.resize(120, 40);
        assert!(h.is_running(), "{q} survives reflow + tick");
        assert!(!h.snapshot().is_empty());
    }
}
