//! Exercises [`ScatterPlot`] the way a dashboard does: two caller-owned point
//! series plotted as a framed correlation cloud with auto-fitted axes, beside
//! an explicitly-bounded panel.
//!
//! The point slices are plain caller-owned state — what an app's model holds
//! and a reducer recomputes; [`ScatterPlot`] only reads them (the same pure
//! projection [`Sparkline`]/[`List`] use). Running over a [`TestBackend`] keeps
//! it TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example scatter_plot_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::Block;
use rstui_widgets::canvas::Marker;
use rstui_widgets::scatter_plot::{ScatterPlot, Series};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(64, 16)).expect("TestBackend is infallible");

    // The two correlations an app's model would own: a roughly-linear trend
    // and a tighter cluster around it.
    let trend: Vec<(f64, f64)> = (0..40)
        .map(|i| {
            let x = f64::from(i);
            (x, 4.0 + 1.7 * x + 6.0 * (x / 3.0).sin())
        })
        .collect();
    let cluster: Vec<(f64, f64)> = (0..24)
        .map(|i| {
            let x = 8.0 + f64::from(i) * 0.9;
            (x, 30.0 + 4.0 * (x / 2.0).cos())
        })
        .collect();

    terminal
        .draw(|frame| {
            let [auto, bounded] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(28)])
                .areas(frame.area());

            // Two series, axes auto-fitted to the data union.
            frame.render_widget(
                ScatterPlot::new([
                    Series::new(&trend, Color::Green),
                    Series::new(&cluster, Color::Magenta).marker(Marker::HalfBlock),
                ])
                .block(Block::bordered().title("latency vs. load"))
                .style(Style::new().fg(Color::Gray)),
                auto,
            );

            // The same trend against fixed bounds: points outside the window
            // clip away (no panic — the totality rule).
            frame.render_widget(
                ScatterPlot::new([Series::new(&trend, Color::Cyan).marker(Marker::Dot)])
                    .x_bounds(Some([0.0, 20.0]))
                    .y_bounds(Some([0.0, 40.0]))
                    .block(Block::bordered().title("clipped 0..20")),
                bounded,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
