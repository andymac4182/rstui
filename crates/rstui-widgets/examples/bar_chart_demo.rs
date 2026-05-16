//! Exercises [`BarChart`] both ways: a framed vertical chart of caller-owned
//! category values and a horizontal one beside it, including a fractional
//! sub-cell boundary glyph.
//!
//! The bars are plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`BarChart`] only reads them (the pure projection
//! [`List`]/[`Gauge`] use). Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example bar_chart_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Bar, BarChart, BarChartDirection, Block};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(44, 10)).expect("TestBackend is infallible");

    // The per-category values an app's model would own.
    let by_lang = || {
        [
            Bar::new(42, "Rust"),
            Bar::new(30, "Go"),
            Bar::new(17, "TS"),
            Bar::new(9, "Sh"),
        ]
    };

    terminal
        .draw(|frame| {
            let [vert, horiz] = Layout::horizontal([Constraint::Length(22), Constraint::Fill(1)])
                .areas(frame.area());

            // Vertical bars with labels on the bottom row, auto-scaled.
            frame.render_widget(
                BarChart::new(by_lang())
                    .bar_width(3)
                    .bar_gap(1)
                    .bar_style(Style::new().fg(Color::Magenta))
                    .block(Block::bordered().title("commits")),
                vert,
            );

            // Horizontal bars with a left label column, capped so the boundary
            // lands mid-cell and renders a partial eighth-block glyph.
            frame.render_widget(
                BarChart::new(by_lang())
                    .direction(BarChartDirection::Horizontal)
                    .max(Some(50))
                    .bar_gap(0)
                    .bar_style(Style::new().fg(Color::Blue))
                    .block(Block::bordered().title("share /50")),
                horiz,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
