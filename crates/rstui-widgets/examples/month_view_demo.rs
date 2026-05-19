//! Exercises [`MonthView`] the way a calendar app would: a framed month grid
//! whose date facts are caller-owned and whose cells carry a borrowed
//! `&[CalendarEvent]` — single-day chips, a multi-day spanning bar, an all-day
//! event, and a "+N more" overflow.
//!
//! `MonthView` does **no date math** — the `year`, `month`, `day_count`, and
//! the weekday of day-of-month 1 are caller-owned inputs an app's model holds
//! (computed by the reducer or a date crate of the caller's choosing), and the
//! events sit on the caller's opaque integer day axis, exactly the pure
//! projection [`List`](rstui_widgets::List) uses for `selected`. No
//! `chrono`/`time` dependency is pulled in. Running over a [`TestBackend`]
//! keeps it TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example month_view_demo
//! ```

use rstui_core::{Color, Position, Style, Terminal, TestBackend};
use rstui_widgets::{Block, CalendarEvent, MonthView};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(43, 20)).expect("TestBackend is infallible");

    // The date facts an app's model owns: May 2026 has 31 days and the 1st is
    // a Friday (weekday index 5, with 0 = Sunday). The widget does no
    // arithmetic with these — it only lays them out. The events sit on a
    // caller-axis day where day-of-month 1 == axis day 1 (the default).
    let (year, month, day_count, weekday_of_first) = (2026, 5, 31, 5);
    let events = [
        // A multi-day conference spanning the 4th–6th: one continuous bar.
        CalendarEvent::new(1, "Conf")
            .with_day(4)
            .with_end_day(6)
            .with_color(Color::Blue),
        // An all-day holiday on the 1st.
        CalendarEvent::new(2, "Holiday")
            .with_day(1)
            .with_all_day(true)
            .with_color(Color::Green),
        // Timed standups — a 12h time prefix shows on each chip.
        CalendarEvent::new(3, "Standup")
            .with_day(7)
            .with_span(9 * 60, 9 * 60 + 15)
            .with_color(Color::Cyan),
        // Three more on the 7th to force a "+N more" footer in its cell.
        CalendarEvent::new(4, "1:1")
            .with_day(7)
            .with_span(11 * 60, 11 * 60 + 30),
        CalendarEvent::new(5, "Review")
            .with_day(7)
            .with_span(14 * 60, 15 * 60),
        CalendarEvent::new(6, "Retro")
            .with_day(7)
            .with_span(16 * 60, 17 * 60),
    ];

    let view = MonthView::new(year, month, day_count, weekday_of_first)
        .events(&events)
        .today(Some(7))
        .selected(Some(17))
        .max_chips(2)
        .today_style(Style::new().fg(Color::Yellow))
        .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan))
        .weekday_style(Style::new().fg(Color::DarkGray))
        .grid_style(Style::new().fg(Color::DarkGray))
        .block(Block::bordered().title("May 2026"));

    // The accessors are pure functions of the same area + config the render
    // reads, so a click maps back through the identical geometry.
    let area = terminal.backend().buffer().area();
    let probe = view.clone();

    terminal
        .draw(|frame| frame.render_widget(view, frame.area()))
        .expect("TestBackend is infallible");

    let buf = terminal.backend().buffer();

    // The framed border is intact.
    assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
    assert_eq!(buf.get(Position::new(42, 0)).unwrap().symbol, '┐');
    assert_eq!(buf.get(Position::new(42, 19)).unwrap().symbol, '┘');

    // The header is the month name and year, centred over the grid span. The
    // inner is Rect(1,1,41,18); cell_w = 41 / 7 = 5 so the 7-column grid span
    // is 35 and "May 2026" (8 wide) → 1 + (35 - 8) / 2 = 14.
    assert_eq!(buf.get(Position::new(14, 1)).unwrap().symbol, 'M');

    // The weekday row is inside the frame on inner row 1 (buffer row 2).
    assert_eq!(buf.get(Position::new(1, 2)).unwrap().symbol, 'S');

    // Day-of-month 1 (Friday) lands in weekday column 5. The first week row
    // is inner row 2 (buffer row 3); the two-digit number is right-aligned so
    // '1' is the second glyph of the cell (cell_x = 1 + 5*5 = 26 → x = 27).
    let cell_w = 41 / 7;
    let day1_cell_x = 1 + 5 * cell_w;
    let day1_num_x = day1_cell_x + 1;
    assert_eq!(buf.get(Position::new(day1_num_x, 3)).unwrap().symbol, '1');

    // The all-day "Holiday" on the 1st renders as a full-cell-width spanning
    // bar on the row below its number; the title starts at the cell's left
    // edge and the bar fill is tinted green.
    assert_eq!(buf.get(Position::new(day1_cell_x, 4)).unwrap().symbol, 'H');
    assert_eq!(
        buf.get(Position::new(day1_cell_x, 4)).unwrap().fg,
        Color::Green
    );

    // The hit-test reports day-of-month 1 for a pointer in that cell, and the
    // all-day event's id for the bar row below the number.
    assert_eq!(probe.day_at(area, Position::new(day1_num_x, 3)), Some(1));
    assert_eq!(probe.event_at(area, Position::new(day1_cell_x, 4)), Some(2));

    print!("{}", terminal.backend());
}
