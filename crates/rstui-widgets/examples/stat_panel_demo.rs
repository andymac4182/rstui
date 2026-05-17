//! Exercises [`StatPanel`]: a row of framed Grafana-style single-stat KPI
//! tiles — requests/s, error rate, p99 latency, uptime — each with a caption,
//! a big value, a `glyph delta` trend row, and a faint sparkline backdrop.
//!
//! The metrics and their trend series are plain caller-owned state — what an
//! app's model holds and a reducer recomputes; [`StatPanel`] only reads them
//! (the pure projection [`List`]/[`Card`] use). The caller, not the widget,
//! decides a rising error rate is red while rising throughput is green.
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example stat_panel_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, StatPanel, Trend};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(72, 7)).expect("TestBackend is infallible");

    // The per-KPI trend series an app's model would own (newest last).
    let req_series = [9u64, 11, 10, 12, 11, 13, 12];
    let err_series = [5u64, 4, 6, 5, 3, 4, 4];
    let p99_series = [120u64, 140, 130, 160, 150, 170, 182];
    let up_series = [100u64, 100, 100, 100, 100, 100, 100];

    terminal
        .draw(|frame| {
            let [reqs, errs, p99, uptime] = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Fill(1),
            ])
            .areas(frame.area());

            // Throughput up is good → green delta.
            frame.render_widget(
                StatPanel::new("12.4k")
                    .caption("Requests/s")
                    .delta("+3%")
                    .trend(Trend::Up)
                    .sparkline(&req_series)
                    .value_style(Style::new().fg(Color::White))
                    .trend_style(Style::new().fg(Color::Green))
                    .spark_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("throughput")),
                reqs,
            );

            // Error rate down is good → green delta on a Down trend.
            frame.render_widget(
                StatPanel::new("0.42%")
                    .caption("Error rate")
                    .delta("-0.1%")
                    .trend(Trend::Down)
                    .sparkline(&err_series)
                    .value_style(Style::new().fg(Color::White))
                    .trend_style(Style::new().fg(Color::Green))
                    .spark_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("errors")),
                errs,
            );

            // Latency up is bad → red delta.
            frame.render_widget(
                StatPanel::new("182 ms")
                    .caption("p99 latency")
                    .delta("+9 ms")
                    .trend(Trend::Up)
                    .sparkline(&p99_series)
                    .value_style(Style::new().fg(Color::White))
                    .trend_style(Style::new().fg(Color::Red))
                    .spark_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("p99")),
                p99,
            );

            // Uptime flat → grey delta.
            frame.render_widget(
                StatPanel::new("99.98%")
                    .caption("Uptime")
                    .delta("0")
                    .trend(Trend::Flat)
                    .sparkline(&up_series)
                    .value_style(Style::new().fg(Color::White))
                    .trend_style(Style::new().fg(Color::Gray))
                    .spark_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("uptime")),
                uptime,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
