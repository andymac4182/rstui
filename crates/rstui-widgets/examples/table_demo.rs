//! Exercises [`Table`] the way a real data grid will: a framed table with a
//! styled header, [`Constraint`]-sized columns, a selection bar, and a `> `
//! gutter, beside an offset-scrolled log-style table with no selection.
//!
//! `selected` and `offset` are plain values here, exactly as they would be
//! fields of an app's model — [`Table`] only ever reads them. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the table layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example table_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Row, Table};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(54, 7)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let [grid, log] =
                Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                    .areas(frame.area());

            // A selectable data grid: caller-owned `selected` drives the bar,
            // a fixed header labels the Constraint-sized columns.
            frame.render_widget(
                Table::new(
                    [
                        Row::new(["init", "ok"]),
                        Row::new(["build", "ok"]),
                        Row::new(["deploy", "fail"]),
                    ],
                    [Constraint::Fill(1), Constraint::Length(4)],
                )
                .header(
                    Row::new(["stage", "stat"]).style(
                        Style::new()
                            .fg(Color::Yellow)
                            .add_modifier(rstui_core::Modifier::BOLD),
                    ),
                )
                .block(Block::bordered().title("pipeline"))
                .highlight_symbol("> ")
                .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                .selected(Some(2)),
                grid,
            );

            // A scrolled, unselected log table: `offset` is just app state too.
            frame.render_widget(
                Table::new(
                    [
                        Row::new(["0", "boot"]),
                        Row::new(["1", "load"]),
                        Row::new(["2", "serve"]),
                        Row::new(["3", "idle"]),
                    ],
                    [Constraint::Length(2), Constraint::Fill(1)],
                )
                .block(Block::bordered().title("log"))
                .style(Style::new().fg(Color::DarkGray))
                .offset(2),
                log,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
