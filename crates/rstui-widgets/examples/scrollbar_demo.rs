//! Exercises [`Scrollbar`] the way a real scrolling pane will: a framed,
//! vertically scrolled [`Paragraph`] with a [`Scrollbar`] drawn down its right
//! border column, so the thumb position reflects how far the text is scrolled.
//!
//! `offset` is a plain value here, exactly as it would be a field of an app's
//! model — both the [`Paragraph`] *and* the [`Scrollbar`] only ever read it
//! (the same pure projection [`List`]/[`Tabs`]/[`Gauge`] use). This is the
//! point of the widget: the scroll state a `Paragraph` already consumes drives
//! the indicator with no extra bookkeeping. Running over a [`TestBackend`]
//! keeps it TTY-free, so it doubles as a deterministic snapshot smoke test of
//! the scrollbar layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example scrollbar_demo
//! ```

use rstui_core::{Color, Margin, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Paragraph, Scrollbar};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(24, 10)).expect("TestBackend is infallible");

    // The whole document and how far it is scrolled: ordinary app state the
    // reducer would own. Scrolling is just changing `offset` in `update`.
    let lines: Vec<String> = (1..=20).map(|n| format!("entry {n:02}")).collect();
    let body = lines.join("\n");
    let offset: usize = 7;

    terminal
        .draw(|frame| {
            let area = frame.area();

            // The text pane: a Paragraph scrolled by `offset`, framed so the
            // scrollbar has a border column to live on.
            frame.render_widget(
                Paragraph::new(body.as_str())
                    .scroll((0, offset as u16))
                    .block(Block::bordered().title("log")),
                area,
            );

            // The scrollbar over the *same* area: it picks the right edge
            // column itself (the block's `│`), inset by one row so it does not
            // clobber the corner glyphs. content_length / position are the
            // exact values the Paragraph scrolled by — one source of truth.
            frame.render_widget(
                Scrollbar::default()
                    .content_length(lines.len())
                    .position(offset)
                    .style(Style::new().fg(Color::DarkGray))
                    .thumb_style(Style::new().fg(Color::Cyan)),
                area.inner(Margin::new(0, 1)),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
