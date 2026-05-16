//! The cross-cutting assertions: drive [`SmokeApp`] through both the headless
//! `Harness` and the *real* `rstui_runtime::run` loop (over an in-memory
//! backend and scripted input), proving the core + runtime + widget public
//! contract still composes. A break here means a stream changed a shared
//! public surface — exactly the signal this crate exists to give.

use rstui_core::{Event, KeyEvent, TestBackend, TestEventSource};
use rstui_runtime::{Harness, run};
use rstui_smoke::{SmokeApp, SmokeMessage};

/// Key-press [`Event`] for `c`, the way the runtime delivers input.
fn key(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}

#[test]
fn harness_composes_core_runtime_and_widgets() {
    // Harness::new runs init + the first render: this single line already
    // exercises rstui-runtime (App/Harness) + rstui-core (Frame/Buffer/
    // Widget seam) + rstui-widgets (Block) together.
    let mut harness = Harness::new(SmokeApp::default(), 30, 5);

    assert!(harness.is_running());
    assert_eq!(harness.app().count(), 0);
    let first = harness.snapshot();
    assert!(
        first.contains("rstui smoke - count: 0"),
        "the widget+runtime composition must render the status line; got:\n{first}"
    );
    // The Block border means the framed row is not a blank line — proves the
    // rstui-widgets widget actually painted through the core seam.
    assert!(
        first
            .lines()
            .next()
            .is_some_and(|row| !row.trim().is_empty()),
        "Block border should make the first row non-blank; got:\n{first}"
    );

    // Event → on_event → update → re-render, across all three crates.
    harness.handle(key('i'));
    assert_eq!(harness.app().count(), 1);
    assert!(harness.snapshot().contains("rstui smoke - count: 1"));

    // An unmapped key is inert (runtime routing still composes correctly).
    harness.handle(key('z'));
    assert_eq!(harness.app().count(), 1);

    // Quit settles through the same shared `settle` core run() uses.
    harness.handle(key('q'));
    assert!(!harness.is_running());
    harness.handle(key('i'));
    assert_eq!(harness.app().count(), 1, "input after quit must be ignored");
}

#[test]
fn live_run_loop_drives_app_to_quit_headlessly() {
    // The *production* `run` loop, not the harness — driven over an in-memory
    // backend and scripted input so the real event loop is on the smoke path.
    let mut events = TestEventSource::with_events([key('i'), key('i'), key('q')]);
    let app = match run(SmokeApp::default(), TestBackend::new(30, 5), &mut events) {
        Ok(app) => app,
        Err(_) => panic!("run() errored on an infallible backend/source"),
    };
    assert_eq!(
        app.count(),
        2,
        "run() must fold both increments before the quit"
    );
}

#[test]
fn live_run_loop_stops_when_input_is_drained() {
    // No quit message: the loop must still terminate when the source drains
    // (poll_event(None) -> Ok(None)), the other run() exit path.
    let mut events = TestEventSource::with_events([key('i')]);
    let app = match run(SmokeApp::default(), TestBackend::new(20, 3), &mut events) {
        Ok(app) => app,
        Err(_) => panic!("run() errored on an infallible backend/source"),
    };
    assert_eq!(app.count(), 1);
}

/// Injecting a message straight into the reducer must match event-driven
/// state — the runtime contract the widget layer renders on top of.
#[test]
fn message_path_matches_event_path() {
    let mut via_event = Harness::new(SmokeApp::default(), 24, 3);
    via_event.handle(key('i'));

    let mut via_message = Harness::new(SmokeApp::default(), 24, 3);
    via_message.message(SmokeMessage::Increment);

    assert_eq!(via_event.app().count(), via_message.app().count());
    assert_eq!(via_event.snapshot(), via_message.snapshot());
}
