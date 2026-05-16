//! Exercises [`Kbd`] the way a help line or menu row does: a few inline
//! keycap clusters laid beside their descriptions.
//!
//! The key labels are plain caller-owned data; [`Kbd`] only projects them to
//! bracketed caps and leaves the rest of each row untouched (it is inline, not
//! a bar). Running over a [`TestBackend`] keeps it TTY-free, so it doubles as
//! a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example kbd_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Kbd};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(30, 7)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("Shortcuts");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());

            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);

            let cap = Style::new().fg(Color::Black).bg(Color::Cyan);
            for (row, (keys, sep, desc)) in [
                (vec!["Ctrl", "S"], "+", "Save the file"),
                (vec!["⌃", "⇧", "P"], "", "Command palette"),
                (vec!["Esc"], " ", "Quit"),
            ]
            .into_iter()
            .enumerate()
            {
                let cols = Layout::horizontal([Constraint::Length(12), Constraint::Min(0)])
                    .split(rows[row]);
                frame.render_widget(Kbd::new(keys).separator(sep).key_style(cap), cols[0]);
                frame.render_widget(desc, cols[1]);
            }
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
