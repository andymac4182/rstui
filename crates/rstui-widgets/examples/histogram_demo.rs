//! Exercises [`Histogram`]: a framed request-latency distribution of
//! caller-owned bucket counts with p50/p95/p99 percentile markers overlaid,
//! including a fractional sub-cell boundary glyph.
//!
//! The buckets are plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`Histogram`] only reads them (the pure projection
//! [`List`]/[`Gauge`] use). Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example histogram_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Histogram, HistogramBucket, Percentile};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(44, 10)).expect("TestBackend is infallible");

    // The synthetic per-bucket latency counts an app's model would own.
    let latency = || {
        [
            HistogramBucket::new(8, "≤5"),
            HistogramBucket::new(31, "≤10"),
            HistogramBucket::new(42, "≤25"),
            HistogramBucket::new(19, "≤50"),
            HistogramBucket::new(6, "≤99"),
        ]
    };

    // The p50/p95/p99 markers a reducer would compute alongside the buckets.
    let markers = [
        Percentile::new(0.5, "p50").style(Style::new().fg(Color::Green)),
        Percentile::new(0.95, "p95").style(Style::new().fg(Color::Yellow)),
        Percentile::new(0.99, "p99").style(Style::new().fg(Color::Red)),
    ];

    terminal
        .draw(|frame| {
            let [auto, capped] = Layout::horizontal([Constraint::Length(22), Constraint::Fill(1)])
                .areas(frame.area());

            // Auto-scaled distribution with the percentile markers overlaid.
            frame.render_widget(
                Histogram::new(&latency())
                    .bar_width(3)
                    .bar_gap(1)
                    .bar_style(Style::new().fg(Color::Magenta))
                    .percentiles(&markers)
                    .block(Block::bordered().title("latency ms")),
                auto,
            );

            // The same distribution capped above its peak so the tallest bar
            // lands mid-cell and renders a partial eighth-block glyph.
            frame.render_widget(
                Histogram::new(&latency())
                    .max(Some(60))
                    .bar_width(2)
                    .bar_gap(0)
                    .bar_style(Style::new().fg(Color::Blue))
                    .percentiles(&markers)
                    .block(Block::bordered().title("latency /60")),
                capped,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
