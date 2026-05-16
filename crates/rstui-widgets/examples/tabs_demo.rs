//! Exercises [`Tabs`] the way a real app shell will: a framed tab strip whose
//! highlighted section is caller-owned state, above the panel that section
//! selects.
//!
//! `selected` is a plain value here, exactly as it would be a field of an
//! app's model — [`Tabs`] only ever reads it (the same pure projection
//! [`List`] uses, one axis over). Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test of the
//! tabs layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example tabs_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Paragraph, Tabs};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 6)).expect("TestBackend is infallible");

    // The active section: ordinary app state the reducer would own. The strip
    // reads it; switching tabs is just changing this value in `update`.
    let selected = 1usize;
    let panels = ["the files pane", "the search pane", "the help pane"];

    terminal
        .draw(|frame| {
            let [strip, body] =
                Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(frame.area());

            frame.render_widget(
                Tabs::new(["Files", "Search", "Help"])
                    .block(Block::bordered().title("rstui"))
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .selected(Some(selected)),
                strip,
            );

            frame.render_widget(
                Paragraph::new(format!("Showing {}.", panels[selected])).block(Block::bordered()),
                body,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
