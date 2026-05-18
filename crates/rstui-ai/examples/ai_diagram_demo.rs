//! The diagram-DSL loop: an agent answers *with a diagram*.
//!
//! This is the worked proof that an AI tool can output a diagram and the
//! client renders it. The model owns one thing — the agent's latest turn
//! string (the exact shape a model emits: prose with a fenced
//! ```` ```mermaid ````/```` ```structurizr ```` block). The view
//! [`Diagram::extract`](rstui_ai::diagram::Diagram::extract)s the diagram
//! from that turn and renders it inside an [`Artifact`] panel — a pure
//! projection, no callbacks, no retained tree (ADR 0012/0017). Driven by
//! the headless [`Harness`] so it is TTY-free and doubles as a
//! deterministic snapshot smoke test.
//!
//! The contract the agent follows to produce that turn is advertised by
//! `rstui_jsonui::capability::DIAGRAM_DSL_NOTE` / `diagram_capability()`
//! (asserted in that crate's tests) — this example is the *render* half.
//!
//! ```text
//! cargo run -p rstui-ai --example ai_diagram_demo
//! ```

use rstui_ai::artifact::Artifact;
use rstui_ai::diagram::{Diagram, DiagramLanguage};
use rstui_core::{KeyCode, KeyEvent};
use rstui_runtime::{App, Cmd, Event, Frame, Harness};

/// Two agent turns, each "answering with a diagram" in the DSL a model
/// emits unprompted — a Mermaid flowchart and a Structurizr C4 workspace.
const TURNS: &[&str] = &[
    "Here's the request flow:\n\n\
     ```mermaid\n\
     flowchart LR\n  U[User] -->|HTTP| API[API]\n  API --> DB[(Database)]\n\
     ```\n\nWant me to add caching?",
    "The C4 context:\n\n\
     ```structurizr\n\
     workspace \"Shop\" {\n  model {\n    u = person \"Customer\"\n    \
     s = softwareSystem \"Storefront\"\n    u -> s \"Buys via\"\n  }\n  \
     views {\n    systemContext s {\n      include *\n    }\n  }\n}\n\
     ```\n",
    // The model controls the exact layout itself — JSON Canvas, explicit
    // x/y/width/height (auto-layout DSLs cannot place a node at a point).
    "I placed the boxes exactly:\n\n\
     ```canvas\n\
     {\"nodes\":[\
     {\"id\":\"in\",\"type\":\"text\",\"text\":\"Ingest\",\"x\":0,\"y\":0,\"width\":160,\"height\":80},\
     {\"id\":\"proc\",\"type\":\"text\",\"text\":\"Process\",\"x\":300,\"y\":0,\"width\":160,\"height\":80},\
     {\"id\":\"out\",\"type\":\"text\",\"text\":\"Store\",\"x\":600,\"y\":0,\"width\":160,\"height\":80}],\
     \"edges\":[\
     {\"id\":\"e1\",\"fromNode\":\"in\",\"toNode\":\"proc\",\"label\":\"raw\"},\
     {\"id\":\"e2\",\"fromNode\":\"proc\",\"toNode\":\"out\",\"label\":\"clean\"}]}\n\
     ```\n",
];

/// What the reducer can do — the Elm message (ADR 0011/0012): a raw event
/// is mapped to one of these, never handled in `view`.
enum Msg {
    /// Show the next agent turn (what a "next answer" affordance would do).
    Next,
    /// Quit.
    Quit,
}

/// The whole screen's state: just which agent turn is shown. The reducer
/// is the only mutation point; the view only reads.
#[derive(Default)]
struct AgentDiagram {
    /// Index into [`TURNS`] of the agent message currently displayed.
    turn: usize,
}

impl App for AgentDiagram {
    type Message = Msg;

    fn on_event(&self, event: Event) -> Option<Msg> {
        let key = event.as_key_press()?;
        match key.code {
            KeyCode::Char(' ') | KeyCode::Tab => Some(Msg::Next),
            KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg) -> Cmd<Msg> {
        match message {
            Msg::Next => self.turn = (self.turn + 1) % TURNS.len(),
            Msg::Quit => return Cmd::quit(),
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let turn = TURNS[self.turn];
        // The agent answered with a diagram — lift it out of the turn.
        let Some(diagram) = Diagram::extract(turn) else {
            return;
        };
        let kind = match diagram.language() {
            DiagramLanguage::Mermaid => "Mermaid",
            DiagramLanguage::Structurizr => "Structurizr (C4)",
            DiagramLanguage::JsonCanvas => "JSON Canvas (explicit placement)",
        };
        let artifact = Artifact::new("Agent diagram").description(kind);
        let body = artifact.body(area);
        frame.render_widget(artifact, area);
        frame.render_widget(diagram, body);
    }
}

fn main() {
    let mut harness = Harness::new(AgentDiagram::default(), 56, 16);
    println!("turn 0 — the agent answered with a ```mermaid block:");
    println!("{}", harness.snapshot());

    harness.handle(Event::from(KeyEvent::char(' ')));
    println!("\nturn 1 — a ```structurizr (C4) block:");
    println!("{}", harness.snapshot());

    harness.handle(Event::from(KeyEvent::char(' ')));
    println!("\nturn 2 — a ```canvas block (the model placed the boxes):");
    let snap = harness.snapshot();
    println!("{snap}");

    // Assert the full loop on the model + the projection, so a regression
    // panics this example (the ai-crate's deterministic-smoke discipline).
    assert_eq!(harness.app().turn, 2, "the reducer advanced the turn");
    let m = Diagram::extract(TURNS[0]).expect("turn 0 carries a diagram");
    assert_eq!(m.language(), DiagramLanguage::Mermaid);
    let c = Diagram::extract(TURNS[1]).expect("turn 1 carries a diagram");
    assert_eq!(c.language(), DiagramLanguage::Structurizr);
    let j = Diagram::extract(TURNS[2]).expect("turn 2 carries a diagram");
    assert_eq!(j.language(), DiagramLanguage::JsonCanvas);
    assert!(
        snap.contains("Agent diagram") && snap.contains("Ingest") && snap.contains("Store"),
        "the JSON Canvas placed nodes rendered inside the artifact:\n{snap}"
    );

    harness.handle(Event::from(KeyEvent::from_code(KeyCode::Esc)));
    println!(
        "\n✓ agent turn → Diagram::extract → rendered; languages: \
         mermaid + structurizr + jsoncanvas (explicit placement), deterministic"
    );
}
