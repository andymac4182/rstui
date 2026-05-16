//! Draws a bordered, titled box into a [`Buffer`], then flushes it through the
//! [`Backend`] boundary exactly the way the runtime will: diff the frame
//! against a blank screen and hand only the changed cells to a backend.
//!
//! Using [`TestBackend`] keeps this free of any raw terminal mode, so it
//! doubles as a deterministic smoke test of the core primitives:
//!
//! ```text
//! cargo run -p rstui-core --example buffer_demo
//! ```

use rstui_core::backend::{Backend, TestBackend};
use rstui_core::{Buffer, Color, Modifier, Position, Rect, Style};

fn draw_border(buf: &mut Buffer, area: Rect) {
    let plain = Style::new();
    let (l, t, r, b) = (area.left(), area.top(), area.right() - 1, area.bottom() - 1);

    for x in area.left()..area.right() {
        buf.set_str(Position::new(x, t), "─", plain);
        buf.set_str(Position::new(x, b), "─", plain);
    }
    for y in area.top()..area.bottom() {
        buf.set_str(Position::new(l, y), "│", plain);
        buf.set_str(Position::new(r, y), "│", plain);
    }
    buf.set_str(Position::new(l, t), "┌", plain);
    buf.set_str(Position::new(r, t), "┐", plain);
    buf.set_str(Position::new(l, b), "└", plain);
    buf.set_str(Position::new(r, b), "┘", plain);
}

fn main() {
    let area = Rect::new(0, 0, 28, 7);
    let mut buf = Buffer::empty(area);

    draw_border(&mut buf, area);

    buf.set_str(
        Position::new(2, 0),
        " rstui-core ",
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    );
    buf.set_str(
        Position::new(2, 2),
        "An idiomatic Rust TUI",
        Style::new().fg(Color::White),
    );
    buf.set_str(
        Position::new(2, 3),
        "rendering substrate.",
        Style::new().fg(Color::White),
    );
    buf.set_str(
        Position::new(2, 5),
        "buffer + geometry + style",
        Style::new().fg(Color::Green).add_modifier(Modifier::DIM),
    );

    // The runtime path: diff the frame against a blank screen and flush only
    // the cells that differ into the backend, then present it.
    let blank = Buffer::empty(area);
    let mut backend = TestBackend::new(area.width, area.height);
    backend
        .draw(buf.diff(&blank))
        .expect("TestBackend is infallible");
    print!("{backend}");
}
