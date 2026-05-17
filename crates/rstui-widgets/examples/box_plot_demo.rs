//! Exercises [`BoxPlot`] the way a stats dashboard does: per-group five-number
//! summaries over a shared scale, horizontal and vertical, with outliers.
//!
//! The summaries are plain caller-owned state — the reducer computes the
//! quartiles in `update` and [`BoxPlot`] only ever reads them (the same pure
//! projection [`List`]/[`BarChart`] use). Running over a [`TestBackend`] keeps
//! it TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example box_plot_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, BoxPlot, BoxPlotOrientation, BoxStats};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(56, 14)).expect("TestBackend is infallible");

    // The per-endpoint latency summaries an app's model would own.
    let latency = [
        BoxStats::new("api/list", 12.0, 28.0, 41.0, 55.0, 80.0).outliers(vec![5.0, 140.0]),
        BoxStats::new("api/get", 8.0, 19.0, 24.0, 33.0, 52.0),
        BoxStats::new("api/post", 30.0, 48.0, 66.0, 90.0, 120.0).outliers(vec![175.0]),
    ];

    terminal
        .draw(|frame| {
            let [horiz, vert] = Layout::vertical([Constraint::Length(8), Constraint::Length(6)])
                .areas(frame.area());

            // Horizontal, auto-scaled over every value (outliers included).
            frame.render_widget(
                BoxPlot::new(latency.clone())
                    .box_style(Style::new().fg(Color::Cyan))
                    .median_style(Style::new().fg(Color::Yellow))
                    .outlier_style(Style::new().fg(Color::Red))
                    .block(Block::bordered().title("latency ms (auto)")),
                horiz,
            );

            // The same data, vertical, against a fixed value window: anything
            // outside 0..150 clamps to the edge (no panic — the totality rule).
            frame.render_widget(
                BoxPlot::new(latency.clone())
                    .orientation(BoxPlotOrientation::Vertical)
                    .bounds(Some([0.0, 150.0]))
                    .box_style(Style::new().fg(Color::Green))
                    .median_style(Style::new().fg(Color::Yellow))
                    .block(Block::bordered().title("window 0..150")),
                vert,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
