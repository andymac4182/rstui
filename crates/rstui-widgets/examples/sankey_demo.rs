//! Exercises [`Sankey`] the way a dashboard does: a left→right flow of a
//! caller-owned request funnel, framed, with proportional node bars and
//! connector bands.
//!
//! The nodes and links are plain caller-owned state — what an app's model
//! holds and a reducer recomputes; [`Sankey`] only ever reads them (the same
//! pure projection [`List`]/[`BarChart`] use). Running over a [`TestBackend`]
//! keeps it TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example sankey_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Sankey, SankeyLink, SankeyNode};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(54, 14)).expect("TestBackend is infallible");

    // A request funnel an app's model would own: visits → signup/bounce →
    // activate/churn, in three left→right columns.
    let nodes = [
        SankeyNode::new(0, "visits"),
        SankeyNode::new(1, "signup"),
        SankeyNode::new(1, "bounce"),
        SankeyNode::new(2, "active"),
        SankeyNode::new(2, "churn"),
    ];
    let links = [
        SankeyLink::new(0, 1, 60),
        SankeyLink::new(0, 2, 40),
        SankeyLink::new(1, 3, 45),
        SankeyLink::new(1, 4, 15),
    ];

    terminal
        .draw(|frame| {
            frame.render_widget(
                Sankey::new(&nodes, &links)
                    .node_width(2)
                    .node_style(Style::new().fg(Color::Cyan))
                    .link_style(Style::new().fg(Color::Blue))
                    .label_style(Style::new().fg(Color::White))
                    .block(Block::bordered().title("request funnel")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
