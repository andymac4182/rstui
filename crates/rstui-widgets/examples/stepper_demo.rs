//! Exercises [`Stepper`] the way a setup wizard does: a horizontal progress
//! rail with the finished steps checked and the current one accented.
//!
//! `current` is plain caller-owned model state an app would hold and a reducer
//! would advance on "Next"; [`Stepper`] only ever reads it and projects the
//! checked/numbered nodes, connectors, and labels. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example stepper_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Step, Stepper};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(46, 3)).expect("TestBackend is infallible");

    // The wizard position an app's model would own: on step 3 of 4.
    let current = 2usize;

    terminal
        .draw(|frame| {
            frame.render_widget(
                Stepper::new([
                    Step::new("Account"),
                    Step::new("Profile"),
                    Step::new("Billing"),
                    Step::new("Done"),
                ])
                .current(current)
                .block(Block::bordered().title("Setup"))
                .done_style(Style::new().fg(Color::Green))
                .current_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                .pending_style(Style::new().fg(Color::DarkGray))
                .connector_style(Style::new().fg(Color::DarkGray)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
