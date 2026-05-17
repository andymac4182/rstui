//! Exercises [`Gantt`] the way a PM dashboard does: a project timeline with
//! one labelled bar per task on a shared integer axis, a progress fill, and a
//! `today` marker, plus an explicitly-ranged release roadmap.
//!
//! The tasks are plain caller-owned state — the plan an app's model would own
//! and a reducer would edit; [`Gantt`] only ever reads it and does **no** date
//! math (the axis units are the caller's, the same pure projection
//! [`List`]/[`Calendar`] use). Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example gantt_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Gantt, GanttTask};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).expect("TestBackend is infallible");

    // A sprint plan an app's model would own. The axis is "day index in the
    // sprint" — the widget never interprets it (no date math).
    let sprint = [
        GanttTask::new(0, 4, "design").progress(100),
        GanttTask::new(3, 8, "build api").progress(60),
        GanttTask::new(6, 12, "build ui").progress(25),
        GanttTask::new(11, 14, "qa").progress(0),
    ];

    // A release roadmap on an explicit quarter axis (0..=12 months).
    let roadmap = [
        GanttTask::new(0, 3, "alpha").progress(100),
        GanttTask::new(2, 7, "beta").progress(40),
        GanttTask::new(6, 12, "ga").progress(0),
    ];

    terminal
        .draw(|frame| {
            let [top, bottom] = Layout::vertical([Constraint::Length(7), Constraint::Length(5)])
                .areas(frame.area());

            // Auto-ranged sprint with a `today` rule at day 7 and a green
            // progress fill over a dim track bar.
            frame.render_widget(
                Gantt::new(sprint.clone())
                    .today(Some(7))
                    .block(Block::bordered().title("sprint 14"))
                    .bar_style(Style::new().fg(Color::DarkGray))
                    .progress_style(Style::new().fg(Color::Green))
                    .today_style(Style::new().fg(Color::Yellow)),
                top,
            );

            // The roadmap on an explicit 0..=12 axis (quarters/months).
            frame.render_widget(
                Gantt::new(roadmap.clone())
                    .range(Some((0, 12)))
                    .block(Block::bordered().title("roadmap (Q axis)"))
                    .bar_style(Style::new().fg(Color::Blue))
                    .progress_style(Style::new().fg(Color::Cyan)),
                bottom,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
