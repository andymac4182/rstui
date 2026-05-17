//! Every Mermaid diagram type, rendered through the one [`Mermaid`] widget.
//!
//! The widget dispatches on the header keyword, so a single read-only widget
//! renders *anything* Mermaid: the original `flowchart` plus `sequenceDiagram`,
//! `classDiagram`, `stateDiagram-v2`, `erDiagram`, `journey`, `gantt`, `pie`,
//! `quadrantChart`, `requirementDiagram`, `gitGraph`, `mindmap`, `timeline`,
//! `sankey-beta`, `xychart-beta`, `block-beta`, `packet-beta`, `kanban`,
//! `architecture-beta`, `radar-beta`, the `C4*` family, and `zenuml` — each a
//! hand-written parser laying out a **deterministic** Unicode diagram (no
//! layout engine, no float jitter: same source, same picture, every time).
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so the gallery wall
//! doubles as a deterministic snapshot smoke of the whole Mermaid layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example mermaid_demo
//! ```

use rstui_core::{Buffer, Position, Rect, Terminal, TestBackend, Widget};
use rstui_widgets::{Block, Mermaid};

/// One labelled fixture per Mermaid diagram type — the canonical source the
/// gallery wall and the per-type smoke share.
const DIAGRAMS: &[(&str, &str)] = &[
    (
        "flowchart",
        "graph TD\n  A[Start] --> B{Valid?}\n  B -->|yes| C(Run)\n  B -->|no| D[Stop]\n  C --> D",
    ),
    (
        "sequenceDiagram",
        "sequenceDiagram\n  participant A as Alice\n  participant B as Bob\n  A->>B: Hello Bob\n  B-->>A: Hi Alice\n  loop healthcheck\n    A->>A: ping\n  end",
    ),
    (
        "classDiagram",
        "classDiagram\n  class Animal {\n    +String name\n    +age int\n    +speak()\n  }\n  class Dog\n  Animal <|-- Dog\n  Dog : +bark()",
    ),
    (
        "stateDiagram-v2",
        "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Running : start\n  Running --> Idle : stop\n  Running --> [*]",
    ),
    (
        "erDiagram",
        "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n  CUSTOMER {\n    string name\n    string email\n  }",
    ),
    (
        "journey",
        "journey\n  title My day\n  section Work\n    Code: 5: Me\n    Review: 3: Me, Bot\n  section Home\n    Sleep: 5: Me",
    ),
    (
        "gantt",
        "gantt\n  title Plan\n  dateFormat YYYY-MM-DD\n  section Build\n    Spec :a1, 2024-01-01, 3d\n    Code :a2, after a1, 5d\n  section Ship\n    Release :crit, after a2, 2d",
    ),
    (
        "pie",
        "pie title Fruit\n  \"Apples\" : 42\n  \"Bananas\" : 30\n  \"Cherries\" : 28",
    ),
    (
        "quadrantChart",
        "quadrantChart\n  title Reach vs effort\n  x-axis Low --> High\n  y-axis Low --> High\n  quadrant-1 Do\n  quadrant-2 Plan\n  quadrant-3 Drop\n  quadrant-4 Delegate\n  A: [0.7, 0.8]\n  B: [0.3, 0.6]",
    ),
    (
        "requirementDiagram",
        "requirementDiagram\n  requirement req1 {\n    id: 1\n    text: must boot\n    risk: high\n  }\n  element e1 {\n    type: test\n  }\n  e1 - satisfies -> req1",
    ),
    (
        "gitGraph",
        "gitGraph\n  commit\n  branch dev\n  checkout dev\n  commit id:\"feat\"\n  checkout main\n  merge dev tag:\"v1\"",
    ),
    (
        "mindmap",
        "mindmap\n  root((rstui))\n    Widgets\n      Mermaid\n      Table\n    Runtime\n      Elm loop",
    ),
    (
        "timeline",
        "timeline\n  title History\n  2002 : LinkedIn\n  2004 : Facebook : Google\n  2006 : Twitter",
    ),
    (
        "sankey-beta",
        "sankey-beta\n  Coal,Electricity,40\n  Gas,Electricity,25\n  Electricity,Homes,35\n  Electricity,Industry,30",
    ),
    (
        "xychart-beta",
        "xychart-beta\n  title Sales\n  x-axis [jan, feb, mar, apr]\n  y-axis \"Revenue\" 0 --> 100\n  bar [30, 55, 40, 80]\n  line [20, 40, 35, 60]",
    ),
    (
        "block-beta",
        "block-beta\n  columns 3\n  A[\"API\"] B[\"Cache\"] C[\"DB\"]\n  A --> B\n  B --> C",
    ),
    (
        "packet-beta",
        "packet-beta\n  0-15: \"Source Port\"\n  16-31: \"Dest Port\"\n  32-63: \"Sequence Number\"\n  64-95: \"Ack Number\"",
    ),
    (
        "kanban",
        "kanban\n  todo[To Do]\n    t1[Spec the API]\n    t2[Draft docs]\n  doing[In Progress]\n    t3[Build parser]\n  done[Done]\n    t4[Scaffold]",
    ),
    (
        "architecture-beta",
        "architecture-beta\n  group api(cloud)[API]\n  service web(server)[Web] in api\n  service db(database)[DB] in api\n  web:R -- L:db",
    ),
    (
        "radar-beta",
        "radar-beta\n  title Skills\n  axis a[\"Rust\"], b[\"TUI\"], c[\"Docs\"], d[\"Tests\"]\n  curve me{4, 5, 3, 5}\n  max 5",
    ),
    (
        "C4Context",
        "C4Context\n  title System\n  Person(user, \"User\", \"a customer\")\n  System(sys, \"App\", \"the product\")\n  Rel(user, sys, \"uses\")",
    ),
    (
        "zenuml",
        "zenuml\n  title Order\n  @Actor User\n  User->Order.create()\n  Order->DB.save()\n  return ok",
    ),
];

/// Render `src` into a fresh `w`×`h` [`Buffer`] and return it as one
/// newline-terminated row per line — the same headless snapshot the widget's
/// own tests use, so this doubles as a determinism check.
fn frame(src: &str, w: u16, h: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    Mermaid::new(src).render(buf.area(), &mut buf);
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push(buf.get(Position::new(x, y)).unwrap().symbol);
        }
        out.push('\n');
    }
    out
}

fn main() {
    // The gallery wall: every diagram type tiled into one screen, each framed
    // and titled by its header keyword — the hero shot proving one widget
    // renders all of Mermaid.
    const COLS: u16 = 3;
    let rows = DIAGRAMS.len().div_ceil(COLS as usize) as u16;
    let (cell_w, cell_h) = (50u16, 11u16);
    let (sw, sh) = (cell_w * COLS, cell_h * rows);

    let mut term = Terminal::new(TestBackend::new(sw, sh)).expect("TestBackend is infallible");
    term.draw(|f| {
        for (i, (label, src)) in DIAGRAMS.iter().enumerate() {
            let cx = (i as u16 % COLS) * cell_w;
            let cy = (i as u16 / COLS) * cell_h;
            f.render_widget(
                Mermaid::new(*src).block(Block::bordered().title(*label)),
                Rect::new(cx, cy, cell_w, cell_h),
            );
        }
    })
    .expect("TestBackend is infallible");
    print!("{}", term.backend());

    // Per-type smoke: each fixture renders deterministically (rendered twice,
    // byte-identical) and routes to its own renderer rather than the legacy
    // "missing graph header" fallback — the contract the dispatcher holds.
    println!("\n── per-type determinism ──");
    for (label, src) in DIAGRAMS {
        let a = frame(src, 60, 16);
        let b = frame(src, 60, 16);
        let det = if a == b {
            "deterministic"
        } else {
            "NON-DETERMINISTIC"
        };
        let routed = if a.contains("missing graph header") {
            "UNROUTED"
        } else {
            "routed"
        };
        println!("  {label:<20} {det:<17} {routed}");
    }

    // An unrecognised header still degrades to the long-standing placeholder,
    // never a panic — the backward-compatible floor.
    let unknown = frame("notADiagram\n  x", 40, 1);
    println!("\nunknown header → {}", unknown.trim_end());
}
