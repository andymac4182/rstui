//! Draws a bordered, titled box into a [`Buffer`] and prints the resulting
//! symbol grid to stdout.
//!
//! This needs no raw terminal mode, so it doubles as a deterministic smoke
//! test of the core primitives:
//!
//! ```text
//! cargo run -p rstui-core --example buffer_demo
//! ```

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

fn render_to_string(buf: &Buffer) -> String {
    let area = buf.area();
    let mut out = String::with_capacity((area.area() + area.height as u32) as usize);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            out.push(buf.get(Position::new(x, y)).map_or(' ', |c| c.symbol));
        }
        out.push('\n');
    }
    out
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

    print!("{}", render_to_string(&buf));
}
