//! Exercises [`Gauge`] the way a real app shows progress: a framed bar whose
//! fill is caller-owned state, beside a second bar at sub-cell precision and a
//! bare track with a custom label.
//!
//! `progress` is a plain value here, exactly as it would be a field of an
//! app's model — [`Gauge`] only ever reads it (the same pure projection
//! [`List`]/[`Tabs`] use). Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test of the gauge layer,
//! including the fractional boundary glyph:
//!
//! ```text
//! cargo run -p rstui-widgets --example gauge_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Gauge};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 9)).expect("TestBackend is infallible");

    // The work done so far: ordinary app state the reducer would own. The
    // gauges read it; advancing the task is just changing this in `update`.
    let progress = 0.66_f64;

    terminal
        .draw(|frame| {
            let [whole, fine, bare] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .areas(frame.area());

            // A labelled bar — the percentage reads over the fill, swapped.
            frame.render_widget(
                Gauge::default()
                    .ratio(progress)
                    .block(Block::bordered().title("Download"))
                    .gauge_style(Style::new().fg(Color::Green).bg(Color::Black)),
                whole,
            );

            // The same ratio one column narrower lands the boundary mid-cell,
            // so the bar ends in a partial eighth-block glyph, not a rounded
            // whole column.
            frame.render_widget(
                Gauge::default()
                    .ratio(progress)
                    .label("")
                    .block(Block::bordered().title("Precise")),
                fine,
            );

            // A bare track with a custom centred label and no fill.
            frame.render_widget(
                Gauge::default()
                    .ratio(0.0)
                    .label("idle")
                    .block(Block::bordered().title("Upload")),
                bare,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
