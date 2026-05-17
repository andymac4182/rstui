//! Exercises [`PieChart`] the way a dashboard does: a framed solid disc of
//! caller-owned category weights with a legend, and a donut beside it.
//!
//! The slices are plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`PieChart`] only reads them (the pure projection
//! [`List`]/[`Gauge`] use). Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example pie_chart_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, PieChart, Slice};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 14)).expect("TestBackend is infallible");

    // The per-category weights an app's model would own (disk by mount).
    let by_mount = || {
        [
            Slice::new(48, Color::Cyan, "/"),
            Slice::new(27, Color::Magenta, "/home"),
            Slice::new(15, Color::Yellow, "/var"),
            Slice::new(10, Color::Green, "swap"),
        ]
    };

    terminal
        .draw(|frame| {
            let [solid, hole] = Layout::horizontal([Constraint::Length(34), Constraint::Fill(1)])
                .areas(frame.area());

            // A framed solid disc with the legend column listing each slice's
            // label and percentage.
            frame.render_widget(
                PieChart::new(by_mount())
                    .legend(true)
                    .block(Block::bordered().title("disk usage")),
                solid,
            );

            // The same weights as a donut: a centred hole at half the radius.
            frame.render_widget(
                PieChart::new(by_mount())
                    .donut(Some(0.5))
                    .style(Style::new().bg(Color::Indexed(235)))
                    .block(Block::bordered().title("donut")),
                hole,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
