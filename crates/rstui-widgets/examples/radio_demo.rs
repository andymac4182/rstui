//! Exercises [`Radio`] the way a real settings form will: a framed column of
//! single-line exclusive-choice controls sharing **one** caller-owned chosen
//! index, with one of them focused (the keyboard target).
//!
//! This is the defining radio-button difference from [`Checkbox`]: there is no
//! per-option `bool` the caller toggles independently — there is a *single*
//! `chosen: usize` the model owns, and each option is projected to
//! `selected(i == chosen)`. The exactly-one-selected invariant lives entirely
//! in the caller; [`Radio`] enforces nothing and only ever *reads* the bool,
//! so it composes with the Elm `view(&self)` model exactly like every other
//! rstui widget. The surrounding [`Block`] and [`Layout`] own the frame and
//! vertical placement; each `Radio` is a leaf control. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test of the radio layer:
//!
//! ```text
//! cargo run -p rstui-widgets --example radio_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Radio};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(30, 7)).expect("TestBackend is infallible");

    // The form's state an app's model would own: ONE chosen index across the
    // whole group, and which control the keyboard is aimed at. A reducer moves
    // `chosen` on arrow keys and `focused_index` on `Tab`.
    let options = ["Low", "Medium", "High", "Maximum"];
    let chosen = 1usize;
    let focused_index = 2usize;

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Quality");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            // One row per option: a Layout splits the pane, each Radio is a
            // single-line leaf. Exclusivity is the projection `i == chosen`.
            let rows = Layout::vertical([Constraint::Length(1); 4]).split(inner);
            for (i, (label, row)) in options.iter().zip(rows.iter()).enumerate() {
                frame.render_widget(
                    Radio::new(*label)
                        .selected(i == chosen)
                        .focused(i == focused_index)
                        .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                    *row,
                );
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
