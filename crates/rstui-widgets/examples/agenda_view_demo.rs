//! Exercises [`AgendaView`] the way a scheduler would: a chronological,
//! day-grouped event list (the "schedule" / list calendar view) over
//! caller-owned events, with a [`List`]-style scroll offset and a selected
//! event row.
//!
//! The events and the day→header-text labels are plain caller-owned state —
//! the model an app would own and a reducer would edit; [`AgendaView`] only
//! ever reads it and does **no** date math (the day axis is an opaque integer,
//! exactly the pure projection [`List`]/[`Calendar`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example agenda_view_demo
//! ```

use rstui_core::{Color, Position, Style, Terminal, TestBackend};
use rstui_widgets::{AgendaView, Block, CalendarEvent};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(46, 12)).expect("TestBackend is infallible");

    // The event model an app would own. The day axis here is "day index" —
    // the widget never interprets it (no date math); the human-readable day
    // headers are the caller's `day_labels`, computed by the reducer or a
    // date crate of the caller's choosing.
    let events = [
        CalendarEvent::new(1, "Standup")
            .with_day(0)
            .with_span(9 * 60, 9 * 60 + 15)
            .with_color(Color::Cyan),
        CalendarEvent::new(2, "Design review")
            .with_day(0)
            .with_span(11 * 60, 12 * 60)
            .with_location("Room 4")
            .with_color(Color::Magenta),
        CalendarEvent::new(3, "Lunch")
            .with_day(0)
            .with_span(12 * 60 + 30, 13 * 60 + 30),
        CalendarEvent::new(4, "Conference")
            .with_day(1)
            .with_end_day(3)
            .with_all_day(true)
            .with_color(Color::Green),
        CalendarEvent::new(5, "1:1")
            .with_day(4)
            .with_span(15 * 60, 15 * 60 + 30)
            .with_location("Quiet booth"),
    ];
    let labels = [(0_i64, "Mon 18 May"), (1, "Tue 19 May"), (4, "Fri 22 May")];

    terminal
        .draw(|frame| {
            frame.render_widget(
                AgendaView::new(&events)
                    .day_labels(&labels)
                    .selected(Some(2)) // the "Design review" row
                    .offset(0)
                    .day_header_style(Style::new().fg(Color::Yellow))
                    .time_style(Style::new().fg(Color::DarkGray))
                    .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .block(Block::bordered().title("Agenda")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    let backend = terminal.backend();
    let buf = backend.buffer();

    // Self-asserting smoke: the bordered frame, the first day header, the
    // first event row, the selected-row accent, and the hit-test inverse.
    assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '\u{250c}');
    // Inner row 0 is the "Mon 18 May" day header (col 1, inside the border).
    assert_eq!(buf.get(Position::new(1, 1)).unwrap().symbol, 'M');
    // Inner row 1 is "09:00–09:15  ● Standup".
    assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, '0');
    // Flattened rows: 0 header, 1 Standup, 2 Design review (selected), …;
    // framed by the border, so the selected cyan bar is buffer row 3.
    assert_eq!(buf.get(Position::new(2, 3)).unwrap().bg, Color::Cyan);
    // event_at inverts the layout: row 2 (inner) is the Standup event id 1.
    let view = AgendaView::new(&events)
        .day_labels(&labels)
        .block(Block::bordered());
    assert_eq!(
        view.event_at(buf.area(), Position::new(3, 2)),
        Some(1),
        "the second framed row hit-tests to the Standup event",
    );
    // 3 day headers + 5 events = 8 flattened rows.
    assert_eq!(view.row_count(), 8);

    print!("{backend}");
}
