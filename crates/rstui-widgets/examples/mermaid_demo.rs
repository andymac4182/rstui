//! Renders a small realistic Mermaid flowchart through [`Mermaid`] inside a
//! [`Block`]: shaped nodes (rectangle, round, diamond, circle), a **chained**
//! spine (`A --> B --> C`), a labelled **fan-out** whose `yes`/`no` labels now
//! render on their own child columns (no more `nos` overprint), and the `&`
//! group shorthand fanning one node into two — the supported top-down subset
//! exercised end to end with the orthogonal bus router.
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the Mermaid layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example mermaid_demo
//! ```

use rstui_core::{Terminal, TestBackend};
use rstui_widgets::{Block, Mermaid};

const FLOW: &str = "\
graph TD
  A[Start] --> B(Load config) --> C{Valid?}
  C -->|yes| D[Run pipeline]
  C -->|no| E[Report error]
  D --> F((Done)) & G[Notify]";

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(64, 33)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            frame.render_widget(
                Mermaid::new(FLOW).block(Block::bordered().title("mermaid")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
