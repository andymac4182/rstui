//! Exercises [`Badge`] inline: a row of level-accented pills laid out beside
//! plain text, showing that a badge paints only its own cells (it does not
//! clobber the rest of the row).
//!
//! Each badge's level/label is plain caller-owned state — what an app's model
//! holds for a status chip; the widget only reads it (the pure projection
//! every widget here uses). Running over a [`TestBackend`] keeps it TTY-free,
//! so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example badge_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Badge, BadgeLevel, Block};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 5)).expect("TestBackend is infallible");

    let accent = |c| Style::new().fg(Color::Black).bg(c);

    terminal
        .draw(|frame| {
            let inner = Block::bordered().title("build status");
            let area = inner.inner(frame.area());
            frame.render_widget(inner, frame.area());

            // One row split into label + four inline pills; each badge paints
            // only its pill, so the cells between them stay blank.
            let cols = Layout::horizontal([
                Constraint::Length(8),
                Constraint::Length(7),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Fill(1),
            ])
            .split(area);

            frame.render_widget(Block::new().title("pkg:"), cols[0]);
            frame.render_widget(
                Badge::new("NEW")
                    .level(BadgeLevel::Info)
                    .info_style(accent(Color::Blue)),
                cols[1],
            );
            frame.render_widget(
                Badge::new("PASSED")
                    .level(BadgeLevel::Success)
                    .success_style(accent(Color::Green)),
                cols[2],
            );
            frame.render_widget(
                Badge::new("2 WARN")
                    .level(BadgeLevel::Warning)
                    .warning_style(accent(Color::Yellow)),
                cols[3],
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
