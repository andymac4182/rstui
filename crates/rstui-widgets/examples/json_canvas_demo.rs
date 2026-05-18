//! A [JSON Canvas 1.0](https://jsoncanvas.org/) document rendered through
//! the [`JsonCanvas`] widget — the *explicit-placement* diagram format.
//!
//! Mermaid and Structurizr are auto-layout; JSON Canvas is the complement:
//! every node carries integer `x`/`y`/`width`/`height`, so the *author*
//! (an AI tool, Obsidian Canvas, a human) controls the layout. This places
//! a `group`, `text`, `file` and `link` node at chosen coordinates with a
//! labelled edge, scaled to fit the terminal. Run over a [`TestBackend`] so
//! it is TTY-free and doubles as a deterministic snapshot smoke:
//!
//! ```text
//! cargo run -p rstui-widgets --example json_canvas_demo
//! ```

use rstui_core::{Buffer, Position, Rect, Terminal, TestBackend, Widget};
use rstui_widgets::{Block, JsonCanvas};

/// A small but representative canvas: a labelled group enclosing two placed
/// nodes, plus a file and a link node to the right, and a labelled edge.
/// `r##"…"##` because `"#intro"` contains the `"#` raw-string terminator.
const CANVAS: &str = r##"{
  "nodes":[
    {"id":"grp","type":"group","x":-40,"y":-40,"width":420,"height":260,"label":"Pipeline","color":"5"},
    {"id":"ingest","type":"text","text":"Ingest\nraw events","x":0,"y":0,"width":160,"height":90},
    {"id":"store","type":"text","text":"Store\nParquet","x":220,"y":80,"width":160,"height":90,"color":"4"},
    {"id":"spec","type":"file","file":"docs/spec.md","subpath":"#intro","x":520,"y":-20,"width":220,"height":110},
    {"id":"site","type":"link","url":"https://jsoncanvas.org","x":520,"y":150,"width":220,"height":70}
  ],
  "edges":[
    {"id":"e1","fromNode":"ingest","fromSide":"right","toNode":"store","toSide":"left","label":"ETL","toEnd":"arrow"},
    {"id":"e2","fromNode":"store","toNode":"spec","label":"documented in"}
  ]
}"##;

/// Render the canvas into a fresh `w`×`h` [`Buffer`]; rows joined by `\n` —
/// the same headless snapshot the widget's own tests use (a determinism
/// check).
fn frame(w: u16, h: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    JsonCanvas::new(CANVAS).render(buf.area(), &mut buf);
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
    // Smoke: the canvas parses to the placed model and renders
    // deterministically (byte-identical on a re-render).
    let canvas = JsonCanvas::parse(CANVAS).expect("valid JSON Canvas");
    assert_eq!(canvas.nodes.len(), 5, "group + 2 text + file + link");
    assert_eq!(canvas.edges.len(), 2);
    assert_eq!(frame(72, 22), frame(72, 22), "deterministic");
    println!(
        "✓ JSON Canvas → {} nodes, {} edges, explicit placement, deterministic\n",
        canvas.nodes.len(),
        canvas.edges.len()
    );

    // Hero: the placed canvas framed in a Block (the GIF freezes on this).
    let mut term = Terminal::new(TestBackend::new(76, 24)).expect("TestBackend is infallible");
    term.draw(|f| {
        f.render_widget(
            JsonCanvas::new(CANVAS)
                .block(Block::bordered().title("JSON Canvas · explicit placement")),
            f.area(),
        );
    })
    .expect("TestBackend is infallible");
    print!("{}", term.backend());
}
