//! Exercises [`SplitPane`] the way a real editor/preview view will: a framed
//! horizontal split whose left pane holds a file list and right pane a
//! preview, with a draggable-looking handle on the divider.
//!
//! The split position is a plain `Constraint` value here — exactly as it would
//! be a field of an app's model that the reducer grows/shrinks on a drag or a
//! resize keystroke. [`SplitPane`] only ever *reads* it and hands back the two
//! pane rects; the caller renders its own widgets into them. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the layout layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example split_pane_demo
//! ```

use rstui_core::{Color, Constraint, Style, Terminal, TestBackend};
use rstui_widgets::{Block, List, Paragraph, SplitPane, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 9)).expect("TestBackend is infallible");

    // Caller-owned split position: the reducer would mutate this on a drag.
    let split_at = Constraint::Percentage(40);

    terminal
        .draw(|frame| {
            let split = SplitPane::new(split_at)
                .block(Block::bordered().title("explorer  (drag │ to resize)"))
                .handle('║')
                .divider_style(Style::new().fg(Color::DarkGray));

            // Pure layout: the widget gives back the two rects, the caller
            // renders its own children into them.
            let (files, preview) = split.split(frame.area());
            frame.render_widget(split, frame.area());

            frame.render_widget(
                List::new(["src/", "  main.rs", "  lib.rs", "Cargo.toml", "README.md"])
                    .highlight_symbol("> ")
                    .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .selected(Some(1)),
                files,
            );

            frame.render_widget(
                Paragraph::new(
                    "fn main() {\n    println!(\"a preview pane to the right of \
                     the divider, rendered by the caller\");\n}",
                )
                .wrap(Wrap { trim: false })
                .style(Style::new().fg(Color::DarkGray)),
                preview,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
