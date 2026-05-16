//! Exercises [`Paragraph`] inside a [`Block`] the way a real content pane
//! will: a word-wrapped styled paragraph, a `trim: false` paragraph that keeps
//! its indentation on wrapped continuations, and a vertically-scrolled,
//! right-aligned paragraph.
//!
//! Running over a [`TestBackend`] keeps it TTY-free, so it doubles as a
//! deterministic snapshot smoke test of the paragraph layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example paragraph_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Paragraph, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let block = Block::bordered().title("paragraph");
            let inner = block.inner(frame.area());
            frame.render_widget(block, frame.area());

            let [wrapped, indented, scrolled] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .areas(inner);

            // Soft word wrap, with the paragraph style cascading into glyphs.
            frame.render_widget(
                Paragraph::new("rstui reflows long copy at word boundaries to fit the pane.")
                    .wrap(Wrap { trim: true })
                    .style(Style::new().fg(Color::Cyan)),
                wrapped,
            );

            // trim:false keeps indentation, so continuations align under text.
            frame.render_widget(
                Paragraph::new("  - keep indentation across wrapped continuation rows")
                    .wrap(Wrap { trim: false }),
                indented,
            );

            // Vertical scroll past the first two rows, right-aligned.
            frame.render_widget(
                Paragraph::new("line A\nline B\nline C\nline D")
                    .scroll((0, 2))
                    .right_aligned()
                    .style(Style::new().fg(Color::DarkGray)),
                scrolled,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
