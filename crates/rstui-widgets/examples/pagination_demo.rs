//! Exercises [`Pagination`] the way a table footer or a results pane does: a
//! windowed page strip pinned to the bottom row under some content.
//!
//! `page`/`page_count` are plain caller-owned model state an app would hold
//! and a reducer would move on `PageUp`/click; [`Pagination`] only ever reads
//! them and projects the windowed strip (first/last always shown, the run
//! around the current page, gaps elided to `…`). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example pagination_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Pagination};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 6)).expect("TestBackend is infallible");

    // The pager state an app's model would own: page 5 of 20 (zero-based 4).
    let (page, page_count) = (4usize, 20usize);

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Search results");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let [body, footer] =
                Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
            frame.render_widget("… 412 matches across 20 pages …", body);
            frame.render_widget(
                Pagination::new(page, page_count)
                    .current_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .control_style(Style::new().fg(Color::DarkGray)),
                footer,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
