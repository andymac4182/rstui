//! Exercises [`Alert`] as a persistent banner stack: an error with a wrapped
//! body and an info note, the validation-summary strip a form keeps pinned
//! while the condition holds (unlike a transient [`Toast`]).
//!
//! The level/title/body are plain caller-owned state — what an app's model
//! holds while the condition is true; the widget only reads them and reuses
//! [`Paragraph`] to wrap the body. Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example alert_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Alert, AlertLevel, Block};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let [err, info, _rest] = Layout::vertical([
                Constraint::Length(6),
                Constraint::Length(3),
                Constraint::Fill(1),
            ])
            .areas(frame.area());

            // A persistent error banner with a body that wraps in its area.
            frame.render_widget(
                Alert::new(AlertLevel::Error, "Build failed")
                    .body(
                        "2 errors in crates/rstui-widgets: unresolved import, \
                         mismatched types. Fix and re-run.",
                    )
                    .error_style(Style::new().fg(Color::White).bg(Color::Red))
                    .block(Block::bordered()),
                err,
            );

            // A compact info note (title only).
            frame.render_widget(
                Alert::new(AlertLevel::Info, "Tip: press ? for keybindings")
                    .info_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .block(Block::bordered()),
                info,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
