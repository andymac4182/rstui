//! Drives a hand-written **A2UI v0.10** JSONL stream through an [`App`]:
//! `createSurface` + `updateComponents` (a `Column` of `Text` /
//! `TextField` / `CheckBox` / primary `Button`) + `updateDataModel`,
//! projected onto a [`UiNode`](rstui_jsonui::tree::UiNode) and rendered
//! every frame as a pure projection of the caller-owned
//! [`A2uiSurface`](rstui_jsonui::a2ui::A2uiSurface) — no retained tree
//! (ADR 0012).
//!
//! Interaction is *resolved, not dispatched*: the view records a
//! [`HitMap`](rstui_jsonui::tree::HitMap); a simulated left-click is
//! mapped back to the agent's `client_to_server.json` action via
//! [`A2uiSurface::action_for`](rstui_jsonui::a2ui::A2uiSurface::action_for),
//! and a checkbox click writes its bound value back through the
//! [`DataModel`](rstui_jsonui::value::DataModel) (the widget never
//! mutates). It runs over a [`Harness`]/`TestBackend`, so it is TTY-free
//! and doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-jsonui --example a2ui_demo
//! ```

use rstui_core::{Modifier, MouseButton, MouseEvent, MouseEventKind, Position, Span, Style};
use rstui_jsonui::a2ui::{A2uiClientAction, A2uiSurface};
use rstui_jsonui::tree::HitMap;
use rstui_runtime::{App, Cmd, Event, Frame, Harness};
use serde_json::Value;
use std::cell::RefCell;

/// The agent's stream: open a surface, define a small form, seed data.
const STREAM: &str = r#"
{"version":"v0.10","createSurface":{"surfaceId":"demo","catalogId":"https://a2ui.org/specification/v0_10/basic_catalog.json"}}
{"version":"v0.10","updateComponents":{"surfaceId":"demo","components":[
  {"id":"root","component":"Column","children":["title","name_field","subscribe","send"]},
  {"id":"title","component":"Text","text":{"path":"/heading"},"variant":"h1"},
  {"id":"name_field","component":"TextField","label":"Name","value":{"path":"/form/name"},"placeholder":"your name"},
  {"id":"subscribe","component":"CheckBox","label":"Subscribe","value":{"path":"/form/subscribe"}},
  {"id":"send_label","component":"Text","text":"Send"},
  {"id":"send","component":"Button","child":"send_label","variant":"primary","action":{"event":{"name":"submitForm","context":{"name":{"path":"/form/name"}}}}}
]}}
{"version":"v0.10","updateDataModel":{"surfaceId":"demo","path":"/heading","value":"A2UI Demo"}}
{"version":"v0.10","updateDataModel":{"surfaceId":"demo","path":"/form","value":{"name":"Ada","subscribe":false}}}
"#;

/// What can happen in the demo: the agent stream arrived, or the user
/// clicked somewhere.
enum Msg {
    /// The buffered A2UI stream has been received.
    StreamReceived,
    /// A left-click at a screen position.
    ClickAt(Position),
}

/// The whole app state is the caller-owned A2UI surface plus the last
/// frame's hit map (interior-mutable because `view` takes `&self`, the
/// immediate-mode accessor pattern).
struct Demo {
    surface: A2uiSurface,
    hits: RefCell<HitMap>,
    /// The last client→server action the reducer produced (so the
    /// snapshot can show the resolved wire message).
    last_action: Option<String>,
}

impl App for Demo {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        // The stream would arrive from the ACP transport; here it is a
        // single delivered message.
        Cmd::perform(|| Msg::StreamReceived)
    }

    fn on_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                position,
                ..
            }) => Some(Msg::ClickAt(position)),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::StreamReceived => {
                self.surface.apply_stream(STREAM);
            }
            Msg::ClickAt(position) => {
                let target = self.hits.borrow().at(position).map(str::to_owned);
                if let Some(node_id) = target {
                    match self.surface.action_for(&node_id) {
                        Some(A2uiClientAction::SetData { pointer, value }) => {
                            // Two-way input: the reducer writes back.
                            self.surface.model_mut().set(&pointer, value);
                        }
                        Some(action @ A2uiClientAction::Event { .. }) => {
                            // The reducer would send this to the agent.
                            self.last_action = action
                                .to_client_json(
                                    self.surface.surface_id().unwrap_or("demo"),
                                    "2026-05-18T12:00:00Z",
                                )
                                .map(|wire| compact(&wire));
                        }
                        Some(A2uiClientAction::OpenUrl(url)) => {
                            self.last_action = Some(format!("openUrl {url}"));
                        }
                        None => {}
                    }
                }
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let mut hits = self.hits.borrow_mut();
        hits.clear();
        let node = self.surface.project();
        // Reserve the bottom line for the resolved-action readout.
        let body = rstui_core::Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
        node.render(body, frame.buffer_mut(), &mut hits);

        let footer = rstui_core::Rect::new(
            area.x,
            area.y + area.height.saturating_sub(1),
            area.width,
            1,
        );
        let status = self
            .last_action
            .clone()
            .unwrap_or_else(|| "click the primary button or the checkbox".to_owned());
        rstui_core::Widget::render(
            rstui_widgets::Paragraph::new(rstui_core::Line::from(Span::styled(
                status,
                Style::new().add_modifier(Modifier::DIM),
            ))),
            footer,
            frame.buffer_mut(),
        );
    }
}

/// Compact JSON for the one-line status readout.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn main() {
    let mut harness = Harness::new(
        Demo {
            surface: A2uiSurface::new(),
            hits: RefCell::new(HitMap::new()),
            last_action: None,
        },
        56,
        10,
    );

    // The stream settles in init(); the form is now on screen.
    println!("-- after the A2UI stream --");
    println!("{}", harness.snapshot());

    // Click the checkbox (row 2) — its bound value flips in the model.
    harness.handle(Event::Mouse(MouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        Position::new(1, 2),
        rstui_core::KeyModifiers::NONE,
    )));

    // Click the primary Button (last row of the body) — resolves to the
    // agent's client→server action with the bound context resolved.
    harness.handle(Event::Mouse(MouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        Position::new(1, 8),
        rstui_core::KeyModifiers::NONE,
    )));

    println!("-- after clicking the checkbox, then the Send button --");
    print!("{}", harness.snapshot());
}
