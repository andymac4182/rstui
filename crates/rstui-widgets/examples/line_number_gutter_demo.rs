//! Exercises [`LineNumberGutter`] the way a real code/review pane will: a
//! framed gutter numbering a window of source lines, with a per-row sign
//! column (a `>` on the caret row, `+`/`-` on changed lines) and a per-row
//! style, then the code itself rendered into the gutter's `inner` rect.
//!
//! The first line number, the row count, the signs, and the per-row styles
//! are all plain caller-owned inputs an app's model would hold and a reducer
//! would derive (from a diff, the caret position, …). The gutter owns no
//! state: it draws the rail and, via the [`Block::inner`]-style
//! [`LineNumberGutter::inner`] accessor, hands the content rect back so the
//! caller composes its own code into it. Running over a [`TestBackend`] keeps
//! it TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example line_number_gutter_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, LineNumberGutter};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(36, 7)).expect("TestBackend is infallible");

    // The code window the model would own (lines 41..=45 of some file).
    let code = [
        "fn main() {",
        "    let mut total = 0;",
        "    for n in 1..=10 {",
        "        total += n;",
        "    }",
    ];
    // Caller-derived: a removed line, an added line, the caret row, context.
    let signs = [' ', '-', '+', '>', ' '];
    let row_styles = [
        Style::new(),
        Style::new().fg(Color::Red),
        Style::new().fg(Color::Green),
        Style::new().fg(Color::Yellow),
        Style::new(),
    ];

    terminal
        .draw(|frame| {
            let gutter = LineNumberGutter::new(41, code.len())
                .signs(&signs)
                .row_styles(&row_styles)
                .block(Block::bordered().title("review"))
                .style(Style::new().fg(Color::DarkGray));

            // The Block::inner composition seam: compute, draw, fill content.
            let inner = gutter.inner(frame.area());
            frame.render_widget(gutter, frame.area());
            for (i, line) in code.iter().enumerate() {
                let row = rstui_core::Rect::new(
                    inner.x,
                    inner.y.saturating_add(i as u16),
                    inner.width,
                    1,
                );
                frame.render_widget(*line, row);
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
