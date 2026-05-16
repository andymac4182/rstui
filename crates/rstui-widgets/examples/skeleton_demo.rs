//! Exercises [`Skeleton`] as a loading card: a solid placeholder block (an
//! avatar/thumbnail) beside text-like line bars, the shimmer swept by a
//! caller-owned `tick` — shown at two ticks so the sweep is visible.
//!
//! `tick` is plain caller-owned model state (a `frame.count()` or a field a
//! `Cmd` advances); the widget never reads a wall clock — the
//! [`Spinner`](rstui_widgets::Spinner) caller-owned-tick precedent. Running
//! over a [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example skeleton_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Skeleton};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 7)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("loading…");
            let area = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let cols = Layout::horizontal([Constraint::Length(8), Constraint::Fill(1)]).split(area);

            let dim = Style::new().fg(Color::DarkGray);
            let shimmer = Style::new().fg(Color::White);

            // A solid block placeholder (a thumbnail), tick 1.
            frame.render_widget(
                Skeleton::new().tick(1).style(dim).shimmer_style(shimmer),
                cols[0],
            );

            // Text-like line bars (a paragraph placeholder), a later tick so
            // the swept shimmer column has visibly moved.
            frame.render_widget(
                Skeleton::new()
                    .lines(3)
                    .tick(6)
                    .style(dim)
                    .shimmer_style(shimmer),
                cols[1],
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
