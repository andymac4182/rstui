//! Exercises [`Heatmap`] both ways: a framed glyph-ramp heatmap of synthetic
//! latency-over-time data and a colour-lerp one beside it, sharing the same
//! caller-owned flat grid.
//!
//! The grid is plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`Heatmap`] only reads it (the pure projection
//! [`List`]/[`Gauge`] use). Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example heatmap_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Heatmap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(72, 10)).expect("TestBackend is infallible");

    // 24 columns (hours) × 6 rows (latency buckets): a synthetic
    // latency-over-time matrix an app's model would own, row-major.
    const COLS: usize = 24;
    const ROWS: usize = 6;
    let grid: Vec<f64> = (0..ROWS)
        .flat_map(|r| {
            (0..COLS).map(move |c| {
                // A diurnal bump over time, hotter in the upper buckets.
                let time = (c as f64 / COLS as f64) * std::f64::consts::TAU;
                let bucket = (ROWS - r) as f64;
                (time.sin() * 0.5 + 0.5) * bucket * 10.0
            })
        })
        .collect();

    let rows = ["p99", "p95", "p90", "p75", "p50", "p25"];
    let cols: Vec<&str> = ["00", "", "", "06", "", "", "12", "", "", "18", "", ""]
        .iter()
        .copied()
        .cycle()
        .take(COLS)
        .collect();

    terminal
        .draw(|frame| {
            let [glyphs, colour] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(frame.area());

            // The shade-ramp reading, auto-scaled, with row/column gutters.
            frame.render_widget(
                Heatmap::new(&grid, COLS)
                    .row_labels(&rows)
                    .col_labels(&cols)
                    .label_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("latency ░▒▓█")),
                glyphs,
            );

            // The Grafana colour-block reading: a green→red background lerp on
            // blank cells, capped so the hottest hour saturates.
            frame.render_widget(
                Heatmap::new(&grid, COLS)
                    .min(Some(0.0))
                    .max(Some(60.0))
                    .glyph_ramp(false)
                    .low_color(Color::Rgb(16, 64, 16))
                    .high_color(Color::Rgb(220, 32, 32))
                    .row_labels(&rows)
                    .col_labels(&cols)
                    .label_style(Style::new().fg(Color::Gray))
                    .block(Block::bordered().title("latency /60ms")),
                colour,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
