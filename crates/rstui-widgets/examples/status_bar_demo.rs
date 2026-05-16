//! Exercises [`StatusBar`] the way a real editor will: a bottom strip pinned
//! under the content with a mode + path on the left, a transient message in
//! the centre, and the cursor position on the right.
//!
//! Every segment is plain caller-owned state — the `String`s/`Line`s an app's
//! model would hold and a reducer would update on edit/move. [`StatusBar`]
//! only reads and places them; it owns nothing and decides nothing. A
//! [`Layout`] splits off the single bottom row the bar draws into (the
//! status-bar idiom), and running over a [`TestBackend`] keeps it TTY-free, so
//! this doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example status_bar_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Line, Span, Style, Terminal, TestBackend};
use rstui_widgets::{Block, StatusBar};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 6)).expect("TestBackend is infallible");

    // The state an app's model would own: editor mode, file, a status
    // message, and the caret position. The widget never mutates these.
    let mode = "NORMAL";
    let file = "src/main.rs";
    let message = "saved";
    let (line_no, col_no) = (128u32, 12u32);

    terminal
        .draw(|frame| {
            // Content pane on top, a one-row status strip pinned to the bottom
            // — the canonical Layout the bar is designed for.
            let rows =
                Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(frame.area());
            frame.render_widget(Block::bordered().title("editor"), rows[0]);

            let accent = Style::new().fg(Color::Black).bg(Color::Cyan);
            frame.render_widget(
                StatusBar::new()
                    .style(Style::new().fg(Color::White).bg(Color::Blue))
                    .left(Line::from(vec![
                        Span::styled(format!(" {mode} "), accent),
                        Span::raw(format!(" {file}")),
                    ]))
                    .center(message)
                    .right(format!(" Ln {line_no}, Col {col_no} ")),
                rows[1],
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
