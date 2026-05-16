//! Drives a few frames through [`Terminal`] the way an app's render loop will:
//! a `draw(|frame| …)` closure per frame, using [`Frame::count`] as a
//! deterministic animation clock and letting the double buffer diff away
//! whatever the previous frame left behind.
//!
//! Running over a [`TestBackend`] keeps it free of raw terminal mode, so it
//! doubles as a deterministic smoke test of the frame driver:
//!
//! ```text
//! cargo run -p rstui-core --example terminal_loop
//! ```

use rstui_core::{Color, Modifier, Position, Style, Terminal, TestBackend};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(24, 3)).expect("TestBackend is infallible");

    // A tiny spinner + frame counter: each frame redraws from a blank buffer,
    // so the terminal only sends the cells that actually changed.
    let spinner = ['|', '/', '-', '\\'];

    for _ in 0..spinner.len() {
        let frame = terminal
            .draw(|frame| {
                let n = frame.count();
                let area = frame.area();
                let buf = frame.buffer_mut();

                buf.set_str(
                    area.position(),
                    &format!("{} rstui", spinner[n % spinner.len()]),
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                );
                buf.set_str(
                    Position::new(0, 2),
                    &format!("frame #{n}"),
                    Style::new().fg(Color::Green).add_modifier(Modifier::DIM),
                );

                // Park the cursor after the spinner glyph.
                frame.set_cursor_position(Position::new(1, 0));
            })
            .expect("TestBackend is infallible");

        println!("--- frame {} ---", frame.count);
        print!("{}", terminal.backend());
    }
}
