//! Exercises [`MaskedInput`] the way a sign-in form does: a focused password
//! field projecting a caller-owned [`TextEdit`], masked and then revealed.
//!
//! The [`TextEdit`] is plain caller-owned model state the widget only reads
//! (the reducer owns the edit, exactly as for [`Input`](rstui_widgets::Input));
//! `focused` and the reveal toggle are caller-owned too. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example masked_input_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend, TextEdit};
use rstui_widgets::{Block, MaskedInput};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 6)).expect("TestBackend is infallible");

    // The field state an app's model would own: a typed secret and whether
    // the "show password" eye is toggled on (caller-owned, reducer-moved).
    let password = TextEdit::from_value("hunter2");
    let reveal = false;

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Sign in");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let rows =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);

            let [label, field] =
                Layout::horizontal([Constraint::Length(10), Constraint::Min(0)]).areas(rows[0]);
            frame.render_widget("Password:", label);
            frame.render_widget(
                MaskedInput::new(&password)
                    .focused(true)
                    .unmasked(reveal)
                    .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                field,
            );
            frame.render_widget("(the reducer owns the reveal toggle and the edit)", rows[1]);
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
