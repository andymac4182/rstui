//! Exercises [`Switch`] the way a real settings panel will: a framed column
//! of two-state sliding toggles, one of them focused (the keyboard target),
//! each a pure projection of a caller-owned `on: bool`.
//!
//! `on` and `focused` are plain caller-owned state here — exactly the `bool`
//! fields an app's model would hold and a reducer would flip on `Space` / move
//! on `Tab`. [`Switch`] only ever reads them: it renders a focused control but
//! does not decide *which* control is focused (focus routing is a separate,
//! deliberately deferred concern, ADR 0004). The surrounding [`Block`] and
//! [`Layout`] own the frame and vertical placement; each `Switch` is a leaf
//! control. Running over a [`TestBackend`] keeps it TTY-free, so it doubles as
//! a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example switch_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Switch};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(28, 6)).expect("TestBackend is infallible");

    // The panel's state an app's model would own: which settings are on, and
    // which control the keyboard is aimed at.
    let settings = [
        ("Wi-Fi", true),
        ("Bluetooth", false),
        ("Dark mode", true),
        ("Airplane", false),
    ];
    let focused_index = 2usize;

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Settings");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let rows = Layout::vertical([Constraint::Length(1); 4]).split(inner);
            for (i, ((label, on), row)) in settings.iter().zip(rows.iter()).enumerate() {
                frame.render_widget(
                    Switch::new()
                        .on(*on)
                        .on_label(*label)
                        .off_label(*label)
                        .focused(i == focused_index)
                        .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                    *row,
                );
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
