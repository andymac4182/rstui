//! Exercises [`Flow`] as the pill / tag-cloud row it exists for: a framed
//! panel of variable-width chips that wrap across rows with a horizontal and
//! vertical gap — the `flexWrap:"wrap"` + `gap` shape rstui expresses as a
//! bounded widget, not a flexbox engine (ADR 0012 §2).
//!
//! The items and gaps are caller-owned; [`Flow`] is a pure projection with a
//! [`Flow::layout`] `Rect` accessor (the `Block::inner` discipline). Running
//! over a [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example flow_demo
//! ```

use rstui_core::{Color, Span, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Flow};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(32, 6)).expect("TestBackend is infallible");

    // The tag set an app's model would own — each a styled pill of its own
    // width; Flow packs and wraps them within whatever area it is given.
    let accent = Style::new().fg(Color::Black).bg(Color::Cyan);
    let tags = [
        Span::styled(" rust ", accent),
        Span::styled(" tui ", accent),
        Span::styled(" immediate-mode ", accent),
        Span::styled(" pure ", accent),
        Span::styled(" no-deps ", accent),
        Span::styled(" widgets ", accent),
    ];

    terminal
        .draw(|frame| {
            let outer = Block::bordered().title("tags");
            let inner = outer.inner(frame.area());
            frame.render_widget(outer, frame.area());
            frame.render_widget(Flow::new(tags).gap(1, 1), inner);
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
