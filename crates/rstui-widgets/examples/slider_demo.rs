//! Exercises [`Slider`] the way a real settings panel will: a framed column
//! of horizontal value selectors, one of them focused (the keyboard target),
//! each a pure projection of a caller-owned `value` within its own range.
//!
//! `value` and `focused` are plain caller-owned state here — exactly the
//! fields an app's model would hold and a reducer would nudge on the arrow
//! keys / move on `Tab`. [`Slider`] only ever reads them: it renders a focused
//! control but does not decide *which* control is focused (focus routing is a
//! separate, deliberately deferred concern, ADR 0004). The surrounding
//! [`Block`] and [`Layout`] own the frame and vertical placement; each
//! `Slider` is a leaf control. Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example slider_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Slider};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 6)).expect("TestBackend is infallible");

    // The panel's state an app's model would own: each setting's value/range
    // and which control the keyboard is aimed at.
    let settings = [
        ("Volume", 70.0_f64, 0.0_f64, 100.0_f64, "70"),
        ("Bright", 40.0, 0.0, 100.0, "40"),
        ("Contrast", 55.0, 0.0, 100.0, "55"),
        ("Gamma", 1.8, 0.5, 3.0, "1.8"),
    ];
    let focused_index = 1usize;

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Display");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let rows = Layout::vertical([Constraint::Length(1); 4]).split(inner);
            for (i, ((label, value, min, max, readout), row)) in
                settings.iter().zip(rows.iter()).enumerate()
            {
                frame.render_widget(
                    Slider::new()
                        .range(*min, *max)
                        .value(*value)
                        .label(*label)
                        .value_label(*readout)
                        .focused(i == focused_index)
                        .thumb_style(Style::new().fg(Color::Cyan))
                        .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                    *row,
                );
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
