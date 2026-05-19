//! Exercises [`TimePicker`] the way a booking form does: a focused, **open**
//! `HH:MM` field dropping its opaque [`List`](rstui_widgets::List) of times
//! over the form behind it.
//!
//! `open`, the selected `minute`, the `highlight`/`offset` are plain
//! caller-owned model state — the widget does no date math and pulls no
//! `chrono` (`HH:MM` formatting is clock arithmetic on a caller `u16`). The
//! dropped list is opaque (it clears its cells, the
//! [`Modal`](rstui_widgets::Modal) technique) so the form cannot bleed
//! through, but it is anchored to the field, not centred. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example time_picker_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Position, Rect, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Paragraph, TimePicker, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("TestBackend is infallible");

    // The form's time state an app's model would own: 09:30 picked, the
    // reducer toggled the list open, the keyboard is on the 10:00 row, and
    // the list is offered every 30 min between 08:00 and 17:00.
    let minute: u16 = 9 * 60 + 30;
    let (start, end, step) = (8 * 60, 17 * 60, 30);
    // 08:00, 08:30, 09:00, 09:30, 10:00 → 10:00 is the 4th row (index 4).
    let highlight = 4usize;

    terminal
        .draw(|frame| {
            // The form the open list drops over (must not bleed through).
            let form = Paragraph::new(
                "Name:  Ada Lovelace\nTime:\nGuests: 2\n\n\
                 // the list clears its own region before drawing.",
            )
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(Color::DarkGray))
            .block(Block::bordered().title("Booking"));
            frame.render_widget(form, frame.area());

            // A one-row time field on the "Time:" line; the open list drops
            // directly below it, anchored to the field.
            let inner = Block::bordered().inner(frame.area());
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
            let [_label, field_area] =
                Layout::horizontal([Constraint::Length(7), Constraint::Min(0)]).areas(rows[1]);

            frame.render_widget(
                TimePicker::new(minute)
                    .open(true)
                    .focused(true)
                    .range(start, end)
                    .step_min(step)
                    .highlight(highlight)
                    .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .block(Block::bordered()),
                field_area,
            );
        })
        .expect("TestBackend is infallible");

    // --- Self-asserting checks (the deterministic smoke) ---

    let buf = terminal.backend().buffer().clone();

    // The closed field, drawn on the "Time:" row inside the form border, shows
    // the picked time 09:30 then the right-aligned disclosure marker.
    let inner = Block::bordered().inner(buf.area());
    let field_y = inner.top() + 1;
    let field_x = inner.left() + 7;
    let head: String = (field_x..field_x + 5)
        .map(|x| buf.get(Position::new(x, field_y)).unwrap().symbol)
        .collect();
    assert_eq!(head, "09:30", "the closed field shows the picked time");

    // The opaque list dropped directly below the field: its first row (inside
    // the panel's block border) is the range start 08:00.
    let list_top = field_y + 2; // field row + the panel block's top border
    let first: String = (field_x + 1..field_x + 6)
        .map(|x| buf.get(Position::new(x, list_top)).unwrap().symbol)
        .collect();
    assert_eq!(first, "08:00", "the list starts at the range start");

    // The highlighted 10:00 row (index 4) carries the accent background.
    let hi_y = list_top + highlight as u16;
    assert_eq!(
        buf.get(Position::new(field_x + 1, hi_y)).unwrap().bg,
        Color::Cyan,
        "the highlighted time row is accented"
    );

    // `minute_at` is the inverse of the render: the pointer over that row
    // resolves back to 10:00 (the app maps a click to a reducer action).
    let field_rect = Rect::new(field_x, field_y, inner.width.saturating_sub(7), 1);
    let picker = TimePicker::new(minute)
        .open(true)
        .range(start, end)
        .step_min(step)
        .block(Block::bordered());
    assert_eq!(
        picker.minute_at(field_rect, Position::new(field_x + 1, hi_y)),
        Some(10 * 60),
        "minute_at inverts the dropped-list layout"
    );

    print!("{}", terminal.backend());
}
