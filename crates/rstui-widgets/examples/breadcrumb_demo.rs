//! Exercises [`Breadcrumb`] the way a real file manager will: a deep path
//! strip pinned to the top row — shown in full when it fits, and **eliding
//! its middle to `…`** when the same path is given a narrow row.
//!
//! The path is plain caller-owned state here — the segments an app's model
//! holds; mapping a click to a crumb and navigating there is the reducer's
//! job, never the widget's. [`Breadcrumb`] only ever *reads* the segments and
//! the optional `selected` index. Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test of the
//! breadcrumb layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example breadcrumb_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Line, Rect, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Breadcrumb};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(56, 7)).expect("TestBackend is infallible");

    // The path an app's model would own (one segment per directory).
    let path = [
        Line::raw("home"),
        Line::raw("andy"),
        Line::raw("dev"),
        Line::raw("rstui"),
        Line::raw("crates"),
        Line::raw("rstui-widgets"),
    ];

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Files");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let [wide_row, _, narrow_row] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(inner);

            // The whole path fits: every crumb shown, the leaf emphasized.
            frame.render_widget(
                Breadcrumb::new(&path)
                    .separator_style(Style::new().fg(Color::DarkGray))
                    .emphasis_style(Style::new().fg(Color::Cyan)),
                wide_row,
            );

            // The same path in a narrow 22-wide row: the middle collapses
            // to `…` (first › … › leaf), kept total under the squeeze.
            frame.render_widget(
                Breadcrumb::new(&path)
                    .separator_style(Style::new().fg(Color::DarkGray))
                    .emphasis_style(Style::new().fg(Color::Cyan)),
                Rect::new(narrow_row.x, narrow_row.y, 22, 1),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
