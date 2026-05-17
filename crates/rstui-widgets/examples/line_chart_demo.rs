//! Exercises [`LineChart`]: a framed, multi-series latency panel — two
//! synthetic "p50"/"p99" curves built from a sine over a shared time axis,
//! with the per-series legend on.
//!
//! The points are plain caller-owned state — what an app's model holds and a
//! reducer recomputes from a metrics ring buffer; [`LineChart`] only reads
//! them (the pure projection [`List`]/[`Gauge`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example line_chart_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{AxisBounds, Block, LineChart, Series};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("TestBackend is infallible");

    // The latency samples an app's model would own, recomputed by a reducer
    // from a metrics ring buffer each tick.
    let p50: Vec<(f64, f64)> = (0..=60)
        .map(|i| {
            let t = f64::from(i);
            (t, 40.0 + 18.0 * (t / 9.0).sin())
        })
        .collect();
    let p99: Vec<(f64, f64)> = (0..=60)
        .map(|i| {
            let t = f64::from(i);
            (t, 110.0 + 55.0 * (t / 7.0).sin())
        })
        .collect();

    let series = [
        Series::new("p50", &p50)
            .marker('•')
            .style(Style::new().fg(Color::Cyan)),
        Series::new("p99", &p99)
            .marker('×')
            .style(Style::new().fg(Color::Magenta)),
    ];

    terminal
        .draw(|frame| {
            let [chart] = Layout::horizontal([Constraint::Fill(1)]).areas(frame.area());

            // A framed line chart with explicit axes (a fixed window so the
            // curves do not jump as the model's ring buffer scrolls) and the
            // per-series legend on the top-right rows.
            frame.render_widget(
                LineChart::new(&series)
                    .x_bounds(AxisBounds::new(0.0, 60.0))
                    .y_bounds(AxisBounds::new(0.0, 180.0))
                    .axis_style(Style::new().fg(Color::DarkGray))
                    .legend(true)
                    .block(Block::bordered().title("latency ms")),
                chart,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
