//! Exercises [`List`] the way a real picker pane will: a framed list with a
//! selection bar and a `> ` gutter, beside a scrolled, unselected log-style
//! list whose offset is past the first rows.
//!
//! `selected` and `offset` are plain values here, exactly as they would be
//! fields of an app's model — [`List`] only ever reads them. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the list layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example list_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, List};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let [picker, log] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(frame.area());

            // A single-select picker: caller-owned `selected` drives the bar.
            frame.render_widget(
                List::new(["Open", "Save", "Save As", "Quit"])
                    .block(Block::bordered().title("menu"))
                    .highlight_symbol("> ")
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .selected(Some(1)),
                picker,
            );

            // A scrolled, unselected log: `offset` is just app state too.
            frame.render_widget(
                List::new(["line 0", "line 1", "line 2", "line 3", "line 4", "line 5"])
                    .block(Block::bordered().title("log"))
                    .style(Style::new().fg(Color::DarkGray))
                    .offset(3),
                log,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
