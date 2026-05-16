//! Exercises [`Align`] the way a real centred splash / corner hint will:
//! a fixed-size child positioned within the screen on both axes, the caller
//! rendering its own content into the aligned rect.
//!
//! [`Align`] is pure layout — the [`Modal`] centring math generalized into a
//! reusable accessor (not a `Modal`: it does not clear or trap focus). It owns
//! no state. Running over a [`TestBackend`] keeps it TTY-free, so it doubles
//! as a deterministic snapshot smoke test of the placement:
//!
//! ```text
//! cargo run -p rstui-widgets --example align_demo
//! ```

use rstui_core::{Alignment, Constraint, Terminal, TestBackend};
use rstui_widgets::{Align, Block, Paragraph, VerticalAlignment};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 9)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            // A centred 22×3 splash.
            let splash = Align::new()
                .width(Constraint::Length(22))
                .height(Constraint::Length(3))
                .block(Block::bordered().title("align"));
            let body = splash.inner(frame.area());
            frame.render_widget(splash, frame.area());
            frame.render_widget(Paragraph::new("centred on both axes"), body);

            // A bottom-right corner hint, same primitive, different anchors.
            let hint = Align::new()
                .width(Constraint::Length(12))
                .height(Constraint::Length(1))
                .horizontal(Alignment::Right)
                .vertical(VerticalAlignment::Bottom);
            frame.render_widget(Paragraph::new("q: quit"), hint.rect(frame.area()));
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
