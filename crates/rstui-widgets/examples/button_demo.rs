//! Exercises [`Button`] the way a real confirmation dialog will: a framed
//! prompt above a row of centred action buttons, one of them focused (the
//! keyboard target).
//!
//! `focused` is plain caller-owned state here — the single `bool` (or, for a
//! row, a focused-index) an app's model would hold and a reducer would move on
//! `Tab`/arrows. Unlike [`Checkbox`](rstui_widgets::Checkbox) a [`Button`] has
//! **no data**: it carries nothing, it *triggers* a message on
//! `Enter`/`Space`, which is the reducer's concern — the widget only renders
//! the affordance. It only ever reads `focused` and does not decide *which*
//! button is focused (focus routing is a separate, deliberately deferred
//! concern). The surrounding [`Block`] and [`Layout`] own the frame and
//! placement; each `Button` is a leaf control. Running over a [`TestBackend`]
//! keeps it TTY-free, so it doubles as a deterministic snapshot smoke test of
//! the button layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example button_demo
//! ```

use rstui_core::{Color, Constraint, Direction, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Button, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 5)).expect("TestBackend is infallible");

    // The dialog's state an app's model would own: which action button the
    // keyboard is aimed at (0 = Cancel, 1 = Save).
    let buttons = ["Cancel", "Save"];
    let focused_index = 1usize;

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Confirm");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            // The prompt over the button row.
            let [prompt, row] =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);
            frame.render_widget(Paragraph::new("Save changes before closing?"), prompt);

            // Two equal-width cells; each Button centres its own label and the
            // focused one gets the full-width focus bar.
            let cells = Layout::new(
                Direction::Horizontal,
                [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
            )
            .split(row);
            for (i, (label, cell)) in buttons.iter().zip(cells.iter()).enumerate() {
                frame.render_widget(
                    Button::new(*label)
                        .focused(i == focused_index)
                        .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                    *cell,
                );
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
