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
fn nine_jumps_to_data_grid() {
    // Adding the Data Grid as the 9th Widgets screen shifts the digit map:
    // `9` now lands on it (Chat moved past the digit range — reach it via
    // the command palette, the index-stable navigation the other tests use).
    let mut h = harness();
    h.handle(ch('9'));
    let s = h.snapshot();
    assert!(s.contains("Data Grid"), "Data Grid title renders:\n{s}");
    assert!(s.contains("s sort"), "the grid keymap hint shows:\n{s}");
}

#[test]
fn chat_send_appends_message_and_canned_reply() {
    let mut h = harness();
    goto(&mut h, "chat"); // Chat, content focused (palette → index-stable)
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
    goto(&mut h, "chat"); // palette → index-stable (digit 9 is now Data Grid)
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

#[test]
fn observability_suite_is_a_third_rail_section() {
    let s = harness().snapshot();
    assert!(s.contains("WIDGETS"), "rail still has Widgets:\n{s}");
    assert!(s.contains("EXPERIENCES"), "rail still has Experiences");
    assert!(
        s.contains("OBSERVABILITY"),
        "rail has the new Observability section:\n{s}"
    );
}

#[test]
fn otel_overview_renders_signals_chart_heatmap_and_logs() {
    let mut h = harness();
    goto(&mut h, "observability");
    let s = h.snapshot();
    assert!(s.contains("Request rate"), "a golden-signal tile:\n{s}");
    assert!(s.contains("Throughput vs errors"), "the line chart panel");
    assert!(s.contains("Service health"), "the health heatmap panel");
    assert!(
        s.contains("Recent errors & warnings"),
        "the live log stream panel"
    );
}

#[test]
fn metrics_explorer_renders_distribution_and_heatmap() {
    let mut h = harness();
    goto(&mut h, "metrics");
    let s = h.snapshot();
    assert!(s.contains("Distribution"), "the latency histogram:\n{s}");
    assert!(s.contains("Latency heatmap"), "the latency heatmap");
    // The range strip cycles with Tab.
    h.handle(key(KeyCode::Tab));
    assert!(h.is_running(), "Tab cycles the range without panicking");
}

#[test]
fn trace_explorer_toggles_waterfall_and_flame() {
    let mut h = harness();
    goto(&mut h, "traces");
    let before = h.snapshot();
    assert!(
        before.contains("Span waterfall"),
        "waterfall view:\n{before}"
    );
    assert!(
        before.contains("service.name"),
        "the selected-span attribute table"
    );
    h.handle(ch('f'));
    let after = h.snapshot();
    assert!(
        after.contains("Flame graph"),
        "`f` toggles to the flame graph:\n{after}"
    );
}

#[test]
fn agent_ui_rail_has_its_own_section() {
    let s = harness().snapshot();
    assert!(s.contains("AGENT UI"), "rail has an Agent UI section:\n{s}");
    assert!(s.contains("A2UI"), "A2UI appears in the rail");
}

#[test]
fn a2ui_screen_renders_the_agent_document_beside_its_output() {
    let mut h = harness();
    goto(&mut h, "a2ui");
    let s = h.snapshot();
    // The split exists: the verbatim agent response and its projection.
    assert!(
        s.contains("Agent response") && s.contains("Rendered output"),
        "A2UI screen shows the source⇆output split:\n{s}"
    );
    assert!(
        s.contains("example 1/3"),
        "starts on the first example:\n{s}"
    );
    // The left pane shows the raw A2UI envelope; the right pane shows it
    // rendered (the bound value resolved from `updateDataModel`).
    assert!(s.contains("createSurface"), "the raw A2UI stream is shown");
    assert!(
        s.contains("Create your account") && s.contains("ada@example.com"),
        "the projected, data-bound form rendered:\n{s}"
    );
    // PgUp/PgDn switches examples (edits persist per example).
    h.handle(key(KeyCode::PageDown));
    let s2 = h.snapshot();
    assert!(
        s2.contains("example 2/3") && s2.contains("Ada Lovelace"),
        "PgDn advances to the profile-card example:\n{s2}"
    );
    assert!(h.is_running());
}

#[test]
fn json_render_screen_renders_the_agent_spec_beside_its_output() {
    let mut h = harness();
    goto(&mut h, "json-render");
    let s = h.snapshot();
    assert!(
        s.contains("Agent response") && s.contains("Rendered output"),
        "json-render screen shows the source⇆output split:\n{s}"
    );
    assert!(s.contains("example 1/3"));
    // The code editor shows the raw spec (no soft-wrap — assert a token
    // at a line start, which is visible without horizontal scroll).
    assert!(
        s.contains("\"root\""),
        "the raw spec is shown in the editor"
    );
    // `✔` is produced only by the rendered StatusLine widget (the source
    // JSON has the text but not the glyph) — proof the right pane is a
    // live projection, not an echo of the source.
    assert!(
        s.contains("api-gateway") && s.contains('✔'),
        "the projected StatusLine rendered with its glyph:\n{s}"
    );
    h.handle(key(KeyCode::PageDown));
    let s2 = h.snapshot();
    assert!(
        s2.contains("example 2/3") && s2.contains("Cache hit rate"),
        "PgDn advances to the metrics example:\n{s2}"
    );
    assert!(h.is_running());
}

#[test]
fn editing_the_agent_document_re_renders_the_output_live() {
    // The left pane is the real code-editor widget; the right pane
    // re-projects the *buffer* every frame. Type a stray char at the
    // top of a valid json-render spec → the spec no longer parses → the
    // right pane must change to the engine's placeholder. That can only
    // happen if the output is a live projection of the edited buffer.
    let mut h = harness();
    goto(&mut h, "json-render");
    assert!(
        h.snapshot().contains("api-gateway"),
        "example 1 renders before editing"
    );
    typed(&mut h, "x"); // inserted at the caret (top of the buffer)
    let s = h.snapshot();
    assert!(
        s.contains("invalid json-render"),
        "editing the buffer re-projected to the invalid-document \
         placeholder (the output tracks the edited source live):\n{s}"
    );
    assert!(h.is_running());
}
