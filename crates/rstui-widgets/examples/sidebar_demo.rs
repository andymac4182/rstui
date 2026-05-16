//! Exercises [`Sidebar`] both ways: an expanded navigation rail (icon + label,
//! a group header, a selected row) beside the same rail **collapsed** to a
//! narrow icon-only column — the IDE/file-manager navigation pane.
//!
//! `selected`/`collapsed` are plain caller-owned model state the widget only
//! reads; moving the selection and committing navigation are the reducer's
//! job. It reuses [`List`](rstui_widgets::List) wholesale, so scrolling, the
//! selection bar, and totality are inherited. Running over a [`TestBackend`]
//! keeps it TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example sidebar_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Sidebar, SidebarItem};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("TestBackend is infallible");

    let items = [
        SidebarItem::group("WORKSPACE"),
        SidebarItem::new("Files").icon('*'),
        SidebarItem::new("Search").icon('?'),
        SidebarItem::group("TOOLS"),
        SidebarItem::new("Run").icon('>'),
        SidebarItem::new("Debug").icon('#'),
    ];

    terminal
        .draw(|frame| {
            let cols = Layout::horizontal([Constraint::Length(6), Constraint::Fill(1)])
                .split(frame.area());

            // Collapsed: a narrow icon-only rail (group headers become rules).
            frame.render_widget(
                Sidebar::new(&items)
                    .collapsed(true)
                    .selected(Some(1))
                    .block(Block::bordered())
                    .highlight_style(Style::new().bg(Color::Blue).fg(Color::White))
                    .group_style(Style::new().fg(Color::DarkGray)),
                cols[0],
            );

            // Expanded: icon + label, a styled group header, a selected row.
            frame.render_widget(
                Sidebar::new(&items)
                    .selected(Some(1))
                    .block(Block::bordered().title("nav"))
                    .highlight_style(Style::new().bg(Color::Blue).fg(Color::White))
                    .group_style(Style::new().fg(Color::DarkGray)),
                cols[1],
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
