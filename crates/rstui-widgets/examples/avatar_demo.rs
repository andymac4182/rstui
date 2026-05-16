//! Exercises [`Avatar`] inline: a member list where each row is a small
//! initials swatch on a per-member accent fill beside the member's name,
//! showing that an avatar paints only its own block.
//!
//! Each avatar's initials/accent are plain caller-owned state — whatever the
//! app derived from the member; the widget only reads and centres them (the
//! pure projection every widget here uses; it does no name parsing). Running
//! over a [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example avatar_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Avatar, Block, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("TestBackend is infallible");

    // (initials, name, accent) — plain caller-owned model state.
    let members = [
        ("AM", "Andrew McClenaghan", Color::Blue),
        ("BK", "Bao Kim", Color::Green),
        ("CR", "Carla Ruiz", Color::Magenta),
    ];

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("members");
            let area = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            // A trailing Fill soaks the slack so the member rows stay
            // contiguous at the top of the pane.
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .split(area);
            for (row, (initials, name, accent)) in rows.iter().zip(members) {
                let cols =
                    Layout::horizontal([Constraint::Length(4), Constraint::Fill(1)]).split(*row);
                frame.render_widget(
                    Avatar::new(initials).style(Style::new().fg(Color::Black).bg(accent)),
                    cols[0],
                );
                frame.render_widget(Paragraph::new(name), cols[1]);
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
