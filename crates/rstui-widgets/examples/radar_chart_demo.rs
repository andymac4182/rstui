//! Exercises [`RadarChart`] the way a dashboard does: a framed spider plot
//! comparing two caller-owned series across a handful of named axes.
//!
//! The axes and series are plain caller-owned state — what an app's model
//! holds and a reducer recomputes; [`RadarChart`] only reads them (the pure
//! projection [`List`]/[`Gauge`] use, with the plot itself composed on
//! [`Canvas`]). Running over a [`TestBackend`] keeps it TTY-free, so it
//! doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example radar_chart_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, RadarAxis, RadarChart, RadarSeries};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(46, 22)).expect("TestBackend is infallible");

    // The axes (each with its own full-scale) and the two series an app's
    // model would own — e.g. two services scored across the same SLOs.
    let axes = [
        RadarAxis::new(100.0, "speed"),
        RadarAxis::new(100.0, "uptime"),
        RadarAxis::new(100.0, "scale"),
        RadarAxis::new(100.0, "cost"),
        RadarAxis::new(100.0, "docs"),
    ];
    let edge = [88.0, 99.0, 70.0, 60.0, 92.0];
    let core = [72.0, 95.0, 90.0, 80.0, 55.0];
    let series = [
        RadarSeries::new(&edge, Color::Cyan),
        RadarSeries::new(&core, Color::Magenta),
    ];

    terminal
        .draw(|frame| {
            frame.render_widget(
                RadarChart::new(&axes, &series)
                    .rings(4)
                    .grid_style(Style::new().fg(Color::Indexed(240)))
                    .block(Block::bordered().title("service scorecard")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
