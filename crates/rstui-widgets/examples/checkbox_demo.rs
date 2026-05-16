//! Exercises [`Checkbox`] the way a real settings form will: a framed column
//! of single-line boolean controls, one of them focused (the keyboard target)
//! and some checked.
//!
//! `checked` and `focused` are plain caller-owned state here — exactly the
//! `bool` fields an app's model would hold and a reducer would toggle on
//! `Space`/`Tab`. [`Checkbox`] only ever reads them: it renders a focused
//! control but does not decide *which* control is focused (focus routing is a
//! separate, deliberately deferred concern). The surrounding [`Block`] and
//! [`Layout`] own the frame and vertical placement; each `Checkbox` is a leaf
//! control. Running over a [`TestBackend`] keeps it TTY-free, so it doubles as
//! a deterministic snapshot smoke test of the checkbox layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example checkbox_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Checkbox};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(30, 7)).expect("TestBackend is infallible");

    // The form's state an app's model would own: which options are on, and
    // which control the keyboard is aimed at.
    let options = [
        ("Wrap long lines", true),
        ("Show line numbers", false),
        ("Auto-save", true),
        ("Vim keybindings", false),
    ];
    let focused_index = 1usize;

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Settings");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            // One row per control: a Layout splits the pane, each Checkbox is
            // a single-line leaf the form places.
            let rows = Layout::vertical([Constraint::Length(1); 4]).split(inner);
            for (i, ((label, checked), row)) in options.iter().zip(rows.iter()).enumerate() {
                frame.render_widget(
                    Checkbox::new(*label)
                        .checked(*checked)
                        .focused(i == focused_index)
                        .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                    *row,
                );
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
