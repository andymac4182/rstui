//! Exercises [`DayView`] the way a scheduling TUI shows a focused day: a
//! framed single-day timeline with an hour ruler, an all-day band, overlap-
//! tiled timed events, a now-line, and a selected event.
//!
//! The events are plain caller-owned state — the day an app's model would own
//! and a reducer would edit; [`DayView`] only ever reads it and does **no**
//! date math (the day is a caller-axis integer, the header a caller-formatted
//! string, exactly the pure projection [`Calendar`]/[`Gantt`] use, with no
//! `chrono`/`time` dependency). Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example day_view_demo
//! ```

use rstui_core::{Color, Position, Style, Terminal, TestBackend};
use rstui_widgets::{Block, CalendarEvent, DayView};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(46, 16)).expect("TestBackend is infallible");

    // One day of the schedule an app's model would own. The day axis is the
    // caller's "day 12" — the widget never interprets it (no date math). Two
    // of these overlap (standup vs. 1:1) so the tiler splits them into two
    // side-by-side columns.
    let day = 12;
    let events = [
        CalendarEvent::new(1, "Sprint Planning (all day)")
            .with_day(day)
            .with_all_day(true)
            .with_color(Color::Indexed(53)),
        CalendarEvent::new(2, "Standup")
            .with_day(day)
            .with_span(9 * 60, 9 * 60 + 30)
            .with_color(Color::Indexed(24)),
        CalendarEvent::new(3, "1:1 with Sam")
            .with_day(day)
            .with_span(9 * 60 + 15, 10 * 60)
            .with_color(Color::Indexed(22)),
        CalendarEvent::new(4, "Design Review")
            .with_day(day)
            .with_span(11 * 60, 12 * 60 + 30)
            .with_location("Room 4")
            .with_color(Color::Indexed(94)),
        CalendarEvent::new(5, "Lunch")
            .with_day(day)
            .with_span(12 * 60 + 30, 13 * 60 + 30)
            .with_color(Color::Indexed(58)),
    ];

    terminal
        .draw(|frame| {
            frame.render_widget(
                DayView::new(day)
                    .events(&events)
                    .day_label("Tue 12 May")
                    .hours(8, 14)
                    .now(Some(11 * 60 + 20)) // 11:20, inside the window
                    .selected_event(Some(4))
                    .block(Block::bordered().title("Day"))
                    .header_style(Style::new().fg(Color::White).bg(Color::Indexed(24)))
                    .ruler_style(Style::new().fg(Color::DarkGray))
                    .grid_style(Style::new().fg(Color::Indexed(236)))
                    .all_day_style(Style::new().fg(Color::White))
                    .now_style(Style::new().fg(Color::Red))
                    .selected_style(Style::new().fg(Color::Black).bg(Color::Yellow)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    let backend = terminal.backend();
    let frame = backend.to_string();
    println!("{frame}");

    // Doubles as a deterministic snapshot test: the layout is fully a function
    // of the caller-owned events, so these cells are stable every run.
    let buf = backend.buffer();
    let at = |x: u16, y: u16| buf.get(Position::new(x, y)).unwrap().symbol;

    // The bordered frame and the caller-formatted header on the first inner
    // row.
    assert_eq!(at(0, 0), '┌', "top-left border");
    assert_eq!(at(1, 1), 'T', "header 'Tue 12 May' inside the frame");

    // The all-day band sits below the header; "Sprint Planning" leads it.
    assert_eq!(at(1, 2), 'S', "all-day band chip title");

    // The hour ruler shows the window's first label "08:00" once the band is
    // laid out (header row + one band row → grid starts on inner row 3).
    let ruler: String = (1..6).map(|x| at(x, 3)).collect();
    assert_eq!(ruler, "08:00", "ruler first hour label, got {ruler:?}");

    // The now-line (11:20 inside 08..=14) drew its '▔' rule somewhere in the
    // event column.
    assert!(
        frame.contains('▔'),
        "expected a now-line in the window:\n{frame}"
    );

    // The overlap tiler split Standup (09:00) and the 1:1 (09:15) into two
    // columns, so both event tints appear on the grid.
    let has = |c: Color| buf.cells().iter().any(|cell| cell.bg == c);
    assert!(has(Color::Indexed(24)), "Standup column tint present");
    assert!(
        has(Color::Indexed(22)),
        "overlapping 1:1 column tint present"
    );

    // The selected Design Review carries the selected accent (yellow bg).
    assert!(
        has(Color::Yellow),
        "selected event accent present:\n{frame}"
    );
}
