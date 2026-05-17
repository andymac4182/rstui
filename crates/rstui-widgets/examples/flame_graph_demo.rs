//! Exercises [`FlameGraph`]: a framed flame graph of a synthetic call stack —
//! a root `main` spanning the full axis, two children, and their grandchildren
//! — with one frame [`selected`](FlameGraph::selected) and highlighted.
//!
//! The frames are plain caller-owned state — what an app's model would hold
//! flattened, and a reducer recomputes when a subtree is zoomed; [`FlameGraph`]
//! only reads them (the pure projection [`List`]/[`Tree`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic snapshot
//! smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example flame_graph_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, FlameFrame, FlameGraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(70, 8)).expect("TestBackend is infallible");

    // A flattened synthetic profile an app's model would own: `main` (the
    // full axis), two children, and a grandchild under each.
    let frames = || {
        [
            FlameFrame::new(0, 0, 100, "main").style(Style::new().fg(Color::White)),
            FlameFrame::new(1, 0, 60, "parse").style(Style::new().fg(Color::Yellow)),
            FlameFrame::new(1, 60, 40, "eval").style(Style::new().fg(Color::Cyan)),
            FlameFrame::new(2, 0, 35, "lex").style(Style::new().fg(Color::Green)),
            FlameFrame::new(2, 60, 25, "fold").style(Style::new().fg(Color::Magenta)),
        ]
    };

    terminal
        .draw(|frame| {
            // A flame graph (root `main` on the bottom row), auto-scaled to
            // the widest frame, with the hot `parse` subtree selected.
            frame.render_widget(
                FlameGraph::new(&frames())
                    .total(Some(100))
                    .selected(Some(1))
                    .selected_style(Style::new().bg(Color::Red).fg(Color::White))
                    .block(Block::bordered().title("cpu profile")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
