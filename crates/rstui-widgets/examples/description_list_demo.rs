//! Exercises [`DescriptionList`] as an inspector pane: aligned key→value rows
//! whose long value wraps inside its column by *reusing* [`Paragraph`].
//!
//! The rows are plain caller-owned state — what an app's model holds and a
//! reducer recomputes as the selected object changes; the widget only reads
//! them (the pure projection [`List`]/[`Table`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example description_list_demo
//! ```

use rstui_core::{Color, Constraint, Span, Style, Terminal, TestBackend};
use rstui_widgets::{Block, DescriptionList, DescriptionRow};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 9)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            // The detail rows an app's model would own for the selected item.
            let rows = [
                DescriptionRow::new(
                    Span::styled("Name", Style::new().fg(Color::Cyan)),
                    "rstui-widgets",
                ),
                DescriptionRow::new(
                    Span::styled("Status", Style::new().fg(Color::Cyan)),
                    Span::styled("Ready", Style::new().fg(Color::Green)),
                ),
                DescriptionRow::new(
                    Span::styled("Summary", Style::new().fg(Color::Cyan)),
                    "A pure-projection widget set whose values wrap by reusing \
                     the Paragraph soft-wrap, never a second algorithm.",
                ),
            ];

            frame.render_widget(
                DescriptionList::new(rows)
                    .key_width(Some(Constraint::Length(8)))
                    .column_spacing(2)
                    .block(Block::bordered().title("Inspector")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
