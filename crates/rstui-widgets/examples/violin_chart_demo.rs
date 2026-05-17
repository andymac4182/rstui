//! Exercises [`ViolinChart`] the way a stats dashboard does: the latency
//! *density* of each service side by side — the distribution shape a
//! [`BoxPlot`]'s five-number summary can't show.
//!
//! The density profiles are plain caller-owned state — an app's reducer
//! computes the KDE/histogram in `update`; [`ViolinChart`] only ever reads
//! them (the same pure projection [`List`]/[`BoxPlot`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example violin_chart_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Violin, ViolinChart, ViolinOrientation};

/// A crude caller-side density: a gaussian-ish bump centred at `mu` over 24
/// samples (the kind of profile a reducer's KDE would produce).
fn density(mu: f64, spread: f64) -> Vec<f64> {
    (0..24)
        .map(|i| {
            let x = f64::from(i);
            let z = (x - mu) / spread;
            (-(z * z) / 2.0).exp()
        })
        .collect()
}

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(64, 16)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let [horiz, vert] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(22)])
                .areas(frame.area());

            // Three services' latency densities, shared scale, median ticks.
            let block = Block::bordered().title("latency density (ms)");
            let inner = block.inner(horiz);
            frame.render_widget(block, horiz);
            frame.render_widget(
                ViolinChart::new([
                    Violin::new("api", density(8.0, 3.0)).median(8.0),
                    Violin::new("db", density(13.0, 5.0)).median(13.0),
                    Violin::new("cache", density(4.0, 2.0)).median(4.0),
                ])
                .bounds(Some([0.0, 23.0]))
                .violin_style(Style::new().fg(Color::Cyan))
                .median_style(Style::new().fg(Color::Yellow))
                .label_style(Style::new().fg(Color::Gray)),
                inner,
            );

            // The same first profile, stood vertically.
            let vblock = Block::bordered().title("vertical");
            let vin = vblock.inner(vert);
            frame.render_widget(vblock, vert);
            frame.render_widget(
                ViolinChart::new([
                    Violin::new("p50", density(9.0, 3.0)),
                    Violin::new("p99", density(15.0, 6.0)),
                ])
                .bounds(Some([0.0, 23.0]))
                .orientation(ViolinOrientation::Vertical)
                .violin_style(Style::new().fg(Color::Magenta)),
                vin,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
