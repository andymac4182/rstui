//! Exercises [`CalendarHeatmap`] the way a profile page does: a GitHub-style
//! contribution calendar of a caller-owned per-day count slice, with weekday
//! labels and month-boundary labels, plus a custom-ramp variant.
//!
//! The day slice is plain caller-owned state — the per-day counts an app's
//! model would own and a reducer would refresh; [`CalendarHeatmap`] only ever
//! reads it and does **no** date math (the caller supplies the start weekday
//! and the month columns, the same pure projection [`Calendar`]/[`List`] use).
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example calendar_heatmap_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, CalendarHeatmap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("TestBackend is infallible");

    // ~12 weeks of per-day contribution counts an app's model would own.
    let mut days: Vec<u64> = Vec::new();
    for i in 0..84u64 {
        // A deterministic pseudo-pattern: weekdays busier than weekends.
        let weekday = i % 7;
        let base = (i * 7 + 3) % 11;
        let v = if weekday >= 5 { base / 3 } else { base };
        days.push(v);
    }

    // The caller supplies month-boundary labels (week column, name) so the
    // widget stays date-math-free.
    let months = vec![
        (0, "Mar".to_string()),
        (4, "Apr".to_string()),
        (9, "May".to_string()),
    ];

    terminal
        .draw(|frame| {
            let [top, bottom] = Layout::vertical([Constraint::Length(10), Constraint::Length(2)])
                .areas(frame.area());

            // The default green GitHub ramp, with weekday + month labels;
            // day[0] sits on a Monday (row 0).
            frame.render_widget(
                CalendarHeatmap::new(&days)
                    .start_weekday(0)
                    .weekday_labels(true)
                    .months(months.clone())
                    .block(Block::bordered().title("contributions")),
                top,
            );

            // The same data on a custom blue ramp, compact (no labels).
            let blue = [
                Style::new().fg(Color::DarkGray),
                Style::new().fg(Color::Blue),
                Style::new().fg(Color::Blue),
                Style::new().fg(Color::LightBlue),
                Style::new().fg(Color::LightCyan),
            ];
            frame.render_widget(
                CalendarHeatmap::new(&days)
                    .start_weekday(0)
                    .levels(blue)
                    .cell('●'),
                bottom,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
