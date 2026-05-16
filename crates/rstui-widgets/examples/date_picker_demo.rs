//! Exercises [`DatePicker`] the way a booking form does: a focused, **open**
//! date field dropping its opaque [`Calendar`](rstui_widgets::Calendar) panel
//! over the form behind it.
//!
//! `open`, `selected`, and the date facts (`year`, `month`, `day_count`, the
//! weekday of day 1) are plain caller-owned model state — the widget does no
//! date math and pulls no `chrono`. The panel is opaque (it clears its cells,
//! the [`Modal`](rstui_widgets::Modal) technique) so the form cannot bleed
//! through, but it is anchored to the field, not centred. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example date_picker_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, DatePicker, Paragraph, Wrap};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(30, 12)).expect("TestBackend is infallible");

    // The form's date state an app's model would own: May 2026 has 31 days
    // and the 1st is a Friday (weekday index 5, 0 = Sunday). Day 17 is the
    // committed choice; today is the 14th; the reducer toggled it open.
    let (year, month, day_count, weekday_of_first) = (2026, 5, 31, 5);

    terminal
        .draw(|frame| {
            // The form the open panel drops over (must not bleed through).
            let form = Paragraph::new(
                "Name:  Ada Lovelace\nDate:\nGuests: 2\n\n\
                 // the panel clears its own region before drawing.",
            )
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(Color::DarkGray))
            .block(Block::bordered().title("Booking"));
            frame.render_widget(form, frame.area());

            // A one-row date field on the "Date:" line; the open calendar
            // panel drops directly below it, anchored to the field.
            let inner = Block::bordered().inner(frame.area());
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
            let [_label, field] =
                Layout::horizontal([Constraint::Length(7), Constraint::Min(0)]).areas(rows[1]);

            frame.render_widget(
                DatePicker::new(year, month, day_count, weekday_of_first)
                    .open(true)
                    .focused(true)
                    .selected(Some(17))
                    .today(Some(14))
                    .focus_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .header_style(Style::new().fg(Color::Cyan))
                    .weekday_style(Style::new().fg(Color::DarkGray))
                    .today_style(Style::new().fg(Color::Yellow))
                    .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .block(Block::bordered()),
                field,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
