//! Exercises [`Drawer`] the way a real app will: an **open** left navigation
//! drawer slid over a dimmed base UI, with caller-owned content (a [`List`])
//! drawn into its [`inner`](rstui_widgets::Drawer::inner) rect.
//!
//! `open` is plain caller-owned model state the widget only reads — the
//! reducer toggles it in `update`, never the widget. The dim
//! [`backdrop_style`](rstui_widgets::Drawer::backdrop_style) is the `Modal`
//! scrim, edge-anchored; the panel is opaque (`clear_region`). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example drawer_demo
//! ```

use rstui_core::{Color, Constraint, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Drawer, DrawerSide, List, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            // The base UI the drawer slides over (and dims).
            frame.render_widget(
                Paragraph::new("main content\nbehind the drawer")
                    .block(Block::bordered().title("workspace")),
                frame.area(),
            );

            let drawer = Drawer::new()
                .open(true)
                .side(DrawerSide::Left)
                .size(Constraint::Length(16))
                .block(Block::bordered().title("Navigate"))
                .style(Style::new().bg(Color::Blue).fg(Color::White))
                .backdrop_style(Style::new().fg(Color::DarkGray));
            let inner = drawer.inner(frame.area());
            frame.render_widget(drawer, frame.area());

            // Caller-owned content rendered into the drawer's inner rect.
            frame.render_widget(
                List::new(["Dashboard", "Projects", "Members", "Settings"])
                    .selected(Some(2))
                    .style(Style::new().bg(Color::Blue).fg(Color::White))
                    .highlight_style(Style::new().bg(Color::White).fg(Color::Blue)),
                inner,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
