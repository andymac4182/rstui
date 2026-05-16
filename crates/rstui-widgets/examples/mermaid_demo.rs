//! Renders a realistic Mermaid flowchart through [`Mermaid`] inside a
//! [`Block`], exercising the full supported subset end to end with the
//! orthogonal bus router:
//!
//! - shaped nodes (rectangle, round, diamond, circle) and a **chained**
//!   spine (`A --> B --> C`),
//! - a labelled **fan-out** whose `yes`/`no` labels render on their own
//!   child columns (no `nos` overprint) and the `&` group shorthand,
//! - a **`subgraph`** cluster drawn as a labelled bordered box around its
//!   members,
//! - **`classDef`/`:::`** node skinning (the error path is tinted via the
//!   deterministic CSS→ANSI color map), and
//! - a **`click`** target, surfaced through [`Mermaid::links`] /
//!   [`Mermaid::link_at`] exactly like a Markdown link.
//!
//! A second diagram exercises the rest of the surface: **left-right (`LR`)**
//! direction, every **edge kind** (`-->` / `---` / `-.->` / `==>`), a
//! **routed back-edge** and a **self-loop** (real return paths, no `↺`
//! stub), and a **skip-rank** edge routed around the intervening box. (A
//! `BT`/`RL` header inverts the axis identically — covered by the tests.)
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the whole Mermaid layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example mermaid_demo
//! ```

use rstui_core::{Position, Rect, Terminal, TestBackend};
use rstui_widgets::{Block, Mermaid};

const FLOW: &str = "\
graph TD
  classDef bad fill:#f00,color:#fff
  A[Start] --> B(Load config) --> C{Valid?}
  C -->|no| E[Report error]:::bad
  C -->|yes| D
  subgraph P [Pipeline]
    D[Run pipeline] --> F((Done)) & G[Notify]
  end
  click E \"https://docs.example/errors\"";

/// Left-right direction, every edge kind, a routed back-edge (`D --> A`), a
/// self-loop (`C --> C`), and a rank-skipping edge (`A ==> D`) that routes
/// around `B`/`C` instead of through them.
const FLOW2: &str = "\
flowchart LR
  A[Ingest] --> B[Parse]
  B -.-> C[Validate]
  C === D[Emit]
  C --> C
  D --> A
  A ==> D";

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(70, 34)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            frame.render_widget(
                Mermaid::new(FLOW).block(Block::bordered().title("mermaid: TD")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());

    let mut lr = Terminal::new(TestBackend::new(72, 20)).expect("TestBackend is infallible");
    lr.draw(|frame| {
        frame.render_widget(
            Mermaid::new(FLOW2).block(Block::bordered().title("mermaid: LR, edges, back-edge")),
            frame.area(),
        );
    })
    .expect("TestBackend is infallible");
    print!("{}", lr.backend());

    // The click registry is exposed exactly like Markdown's links: a host
    // tracks a focused index and the reducer turns Enter/click into a
    // `LinkActivation`. Hit-test the rendered error node to prove it.
    let view = Mermaid::new(FLOW).block(Block::bordered());
    let area = Rect::new(0, 0, 70, 34);
    let links = view.links();
    println!("\nclick targets: {links:?}");
    if let Some(region) = view.link_regions(area).first() {
        let hit = Position::new(region.rect.x, region.rect.y);
        if let Some(i) = view.link_at(hit, area) {
            println!(
                "clicking node at {hit:?} activates {:?}",
                links[i].activate(i)
            );
        }
    }
}
