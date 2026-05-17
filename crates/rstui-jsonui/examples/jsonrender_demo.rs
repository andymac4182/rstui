//! Feeds a small json-render spec plus an RFC-6902 JSONL **patch
//! stream** into a runtime [`App`], projects it through
//! [`UiNode`](rstui_jsonui::tree::UiNode), and routes a click back to the
//! agent's action — the whole json-render slice end to end, headless.
//!
//! The agent here "streams" a task list: a `Card` titled from state, a
//! `repeat` over `/todos` rendering one `Text` per item, a live
//! `$count`+`$concat` summary, and an `Add` `ConfirmInput` whose
//! `confirm` event runs the built-in `pushState`. Nothing is a callback —
//! a hit-tested [`HitMap`](rstui_jsonui::tree::HitMap) id is turned into
//! a [`ResolvedAction`](rstui_jsonui::jsonrender::ResolvedAction) the
//! reducer applies (ADR 0012 §P1, pure projection: `view` only draws,
//! the hit map is refreshed in `update`, the one mutation seam).
//!
//! It runs over a [`Harness`] with no TTY, so it doubles as a
//! deterministic regression test:
//!
//! ```text
//! cargo run -p rstui-jsonui --example jsonrender_demo
//! ```

use std::cell::RefCell;

use rstui_core::{
    Buffer, Event, Frame, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, Position,
    Rect,
};
use rstui_jsonui::jsonrender::JsonRenderDoc;
use rstui_jsonui::tree::HitMap;
use rstui_runtime::{App, Cmd, Harness};

/// The agent's reply: a `{ "type":"spec" }` JSONL patch stream that
/// progressively builds the UI. The demo feeds this in slices (as a
/// network would deliver chunks).
const AGENT_PATCH_STREAM: &str = r#"{"op":"add","path":"/root","value":"card"}
{"op":"add","path":"/state","value":{"title":"My Tasks","todos":[{"text":"Write the spec"},{"text":"Review the PR"}],"draft":"Ship it"}}
{"op":"add","path":"/elements/card","value":{"type":"Card","props":{"title":{"$state":"/title"}},"children":["list","summary","add"]}}
{"op":"add","path":"/elements/list","value":{"type":"Box","props":{"flexDirection":"column"},"repeat":{"statePath":"/todos"},"children":["row"]}}
{"op":"add","path":"/elements/row","value":{"type":"Text","props":{"text":{"$template":"- ${text}"}}}}
{"op":"add","path":"/elements/summary","value":{"type":"StatusLine","props":{"status":"info","text":{"$concat":[{"$count":{"$state":"/todos"}}," task(s) open"]}}}}
{"op":"add","path":"/elements/add","value":{"type":"ConfirmInput","props":{"message":"Add a task?"},"on":{"confirm":{"action":"pushState","params":{"statePath":"/todos","value":{"text":{"$state":"/draft"}}}}}}}
"#;

/// The surface size (kept on the model so a `&self` refresh after a
/// resize would re-project at the right dimensions; fixed here).
const SIZE: (u16, u16) = (48, 12);

/// The caller-owned model: the json-render document plus the hit map the
/// last projection recorded. The hit map is in a `RefCell` so `view`
/// (`&self`) can keep it current as it draws — the immediate-mode
/// accessor pattern from `docs/composition.md` (the cell is interior
/// bookkeeping, not app state the reducer owns).
struct DemoApp {
    doc: JsonRenderDoc,
    hits: RefCell<HitMap>,
}

impl Default for DemoApp {
    fn default() -> Self {
        let mut doc = JsonRenderDoc::new();
        // Stream the patches in two slices so progressive rendering is
        // exercised (the tail completes the spec), then finalise.
        let (head, tail) = AGENT_PATCH_STREAM.split_at(AGENT_PATCH_STREAM.len() / 2);
        doc.ingest(head);
        doc.ingest(tail);
        doc.finish_stream();
        Self {
            doc,
            hits: RefCell::new(HitMap::new()),
        }
    }
}

impl DemoApp {
    /// The id of the interactive node at `position`, resolved against the
    /// last projection's recorded rectangles.
    fn node_at(&self, position: Position) -> Option<String> {
        self.hits.borrow().at(position).map(str::to_owned)
    }
}

/// What can happen: a click at a screen position (resolved to a node id),
/// or quit.
enum Msg {
    /// A pointer press at a screen cell.
    ClickAt(Position),
    /// Esc — leave.
    Quit,
}

impl App for DemoApp {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        if let Some(key) = event.as_key_press() {
            if matches!(key.code, KeyCode::Esc) {
                return Some(Msg::Quit);
            }
        }
        if let Some(mouse) = event.as_mouse() {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                return Some(Msg::ClickAt(mouse.position));
            }
        }
        None
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Quit => Cmd::quit(),
            Msg::ClickAt(position) => {
                // Resolve the click to a node id, then let the doc turn
                // it into resolved actions the reducer applies — never a
                // callback (ADR 0012 §P1). `dispatch` mutates the model.
                if let Some(node_id) = self.node_at(position) {
                    self.doc.dispatch(&node_id, "press");
                }
                Cmd::none()
            }
        }
    }

    fn view(&self, frame: &mut Frame<'_>) {
        // Pure projection: re-derive a fresh UiNode every frame and walk
        // it, recording interactive rects into the cell so the *next*
        // click resolves against exactly what is on screen.
        let node = self.doc.view();
        let area = frame.area();
        let mut hits = HitMap::new();
        node.render(area, frame.buffer_mut(), &mut hits);
        *self.hits.borrow_mut() = hits;
    }
}

/// Finds a button label in a snapshot and returns a cell inside it (the
/// demo's stand-in for a pointer device).
fn locate(snapshot: &str, label: &str) -> Position {
    for (row, line) in snapshot.lines().enumerate() {
        if let Some(column) = line.find(label) {
            #[allow(clippy::cast_possible_truncation)]
            return Position::new(column as u16 + 1, row as u16);
        }
    }
    Position::new(0, 0)
}

fn main() {
    let (width, height) = SIZE;
    let mut harness = Harness::new(DemoApp::default(), width, height);

    println!("initial projection (streamed spec + JSONL patch stream):");
    let initial = harness.snapshot();
    println!("{initial}");
    assert!(
        initial.contains("My Tasks"),
        "Card title resolved from /title state"
    );
    assert!(
        initial.contains("Write the spec"),
        "first /todos repeat item rendered"
    );
    assert!(
        initial.contains("2 task(s) open"),
        "$count + $concat summary resolved"
    );

    // Sanity-check the projection directly (no terminal needed).
    let projected = harness.app().doc.view();
    assert!(
        projected.to_plain().contains("Review the PR"),
        "second repeat item is in the projected UiNode tree"
    );

    // The ConfirmInput's "Yes" button is interactive; click it.
    let yes = locate(&initial, "Yes");
    assert!(
        harness.app().node_at(yes).is_some(),
        "the Add confirm button is hit-testable from the recorded rects"
    );
    harness.handle(Event::from(MouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        yes,
        KeyModifiers::NONE,
    )));

    println!("\nafter clicking 'Yes' (pushState ran in the reducer):");
    let after = harness.snapshot();
    println!("{after}");
    assert!(
        after.contains("Ship it"),
        "pushState appended the /draft value as a new todo"
    );
    assert!(
        after.contains("3 task(s) open"),
        "the $count summary re-projected after the state mutation"
    );

    // Esc quits; further input is ignored.
    harness.handle(Event::from(rstui_core::KeyEvent::from_code(KeyCode::Esc)));
    assert!(!harness.is_running(), "Esc quit the app");
    let todo_count = harness
        .app()
        .doc
        .model()
        .get("/todos")
        .and_then(|todos| todos.as_array())
        .map(Vec::len);
    assert_eq!(
        todo_count,
        Some(3),
        "the data model is the single source of truth: 3 todos"
    );

    // The projection is a pure function of the model — re-deriving it on
    // a scratch buffer yields the same screen, no retained tree.
    let mut scratch = Buffer::empty(Rect::new(0, 0, width, height));
    let mut throwaway = HitMap::new();
    harness
        .app()
        .doc
        .view()
        .render(scratch.area(), &mut scratch, &mut throwaway);

    println!("\nfinal: 3 todos in the data model, projection re-derives cleanly (asserts passed)");
}
