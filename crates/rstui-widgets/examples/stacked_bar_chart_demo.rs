//! Exercises [`StackedBarChart`] the way a metrics dashboard does: a stacked
//! composition chart (revenue by region split by product) and the same series
//! as a grouped/clustered chart, plus a horizontal stacked variant.
//!
//! The bars are plain caller-owned state — the aggregates an app's model would
//! own and a reducer would recompute; [`StackedBarChart`] only ever reads them
//! (the same pure projection [`List`]/[`BarChart`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example stacked_bar_chart_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Terminal, TestBackend};
use rstui_widgets::{BarChartDirection, Block, StackMode, StackedBar, StackedBarChart};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(54, 16)).expect("TestBackend is infallible");

    // Quarterly revenue by region, each split into three product segments —
    // the aggregates an app's model would own.
    let series = || {
        vec![
            StackedBar::new(
                "Q1",
                vec![(8, Color::Cyan), (5, Color::Magenta), (3, Color::Yellow)],
            ),
            StackedBar::new(
                "Q2",
                vec![(6, Color::Cyan), (9, Color::Magenta), (4, Color::Yellow)],
            ),
            StackedBar::new(
                "Q3",
                vec![(10, Color::Cyan), (6, Color::Magenta), (7, Color::Yellow)],
            ),
            StackedBar::new(
                "Q4",
                vec![(7, Color::Cyan), (8, Color::Magenta), (9, Color::Yellow)],
            ),
        ]
    };

    terminal
        .draw(|frame| {
            let [top, bottom] = Layout::vertical([Constraint::Length(10), Constraint::Length(6)])
                .areas(frame.area());
            let [stacked, grouped] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(top);

            // Stacked: the segments accumulate up each bar in their own colour.
            frame.render_widget(
                StackedBarChart::new(series())
                    .mode(StackMode::Stacked)
                    .bar_width(3)
                    .block(Block::bordered().title("revenue (stacked)")),
                stacked,
            );

            // Grouped: the same series as adjacent clustered sub-bars.
            frame.render_widget(
                StackedBarChart::new(series())
                    .mode(StackMode::Grouped)
                    .bar_width(3)
                    .block(Block::bordered().title("revenue (grouped)")),
                grouped,
            );

            // A horizontal stacked variant (the same composition, rotated).
            frame.render_widget(
                StackedBarChart::new(series())
                    .mode(StackMode::Stacked)
                    .direction(BarChartDirection::Horizontal)
                    .block(Block::bordered().title("horizontal stacked")),
                bottom,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
