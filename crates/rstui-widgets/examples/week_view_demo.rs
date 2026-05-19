//! Exercises [`WeekView`] the way a scheduler would: a framed Mon–Fri time
//! grid whose events are caller-owned and whose day columns, "now" line, and
//! selected block the widget only projects.
//!
//! `WeekView` does **no date math** — the caller-axis `start_day`, the
//! per-column day labels, and the [`CalendarEvent`] day/minute integers are
//! caller-owned inputs an app's model holds (computed by the reducer or a date
//! crate of the caller's choosing), exactly the pure projection [`List`] uses
//! for `selected`. No `chrono`/`time` dependency is pulled in. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example week_view_demo
//! ```

use rstui_core::{Color, Position, Style, Terminal, TestBackend};
use rstui_widgets::{Block, CalendarEvent, WeekView};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 16)).expect("TestBackend is infallible");

    // The facts an app's model owns: a working week whose column 0 is the
    // caller's axis day 0, with caller-formatted column headers. The widget
    // does no arithmetic with these — it only lays them out.
    let labels = ["Mon", "Tue", "Wed", "Thu", "Fri"];
    let events = [
        CalendarEvent::new(1, "Standup")
            .with_day(0)
            .with_span(9 * 60, 9 * 60 + 30)
            .with_color(Color::Cyan),
        CalendarEvent::new(2, "Design")
            .with_day(1)
            .with_span(10 * 60, 11 * 60 + 30)
            .with_color(Color::Green),
        // Overlaps "Design" → the two tile into side-by-side lanes.
        CalendarEvent::new(3, "1:1")
            .with_day(1)
            .with_span(11 * 60, 12 * 60)
            .with_color(Color::Magenta),
        CalendarEvent::new(4, "Review")
            .with_day(3)
            .with_span(14 * 60, 15 * 60)
            .with_color(Color::Blue),
        // A multi-day trip rides the all-day band across Thu–Fri.
        CalendarEvent::new(5, "Offsite")
            .with_day(3)
            .with_end_day(4)
            .with_all_day(true)
            .with_color(Color::Yellow),
    ];

    terminal
        .draw(|frame| {
            frame.render_widget(
                WeekView::new(0, 5)
                    .day_labels(&labels)
                    .events(&events)
                    .hours(8, 18)
                    .today(Some(2)) // Wednesday is "today"
                    .now(Some(13 * 60 + 30)) // a 13:30 "now" line
                    .selected_event(Some(2))
                    .header_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .ruler_style(Style::new().fg(Color::DarkGray))
                    .all_day_style(Style::new().fg(Color::DarkGray))
                    .now_style(Style::new().fg(Color::Red))
                    .selected_style(Style::new().fg(Color::Black).bg(Color::White))
                    .block(Block::bordered().title("This Week")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    // Deterministic self-asserting smoke: the inner area is Rect(1,1,58,14);
    // the ruler sits at x = 1, the day columns begin 6 columns right of it,
    // and a "now" line at 13:30 sits inside the 08:00–18:00 window.
    let backend = terminal.backend();
    let buf = backend.buffer();

    // The frame is drawn.
    assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '┌');
    // The hour ruler's first label is "08:00" at the first grid row (inner
    // row 1 = header, row 2 = all-day band, row 3 = first grid row → y = 3).
    let ruler: String = (1..6)
        .map(|x| buf.get(Position::new(x, 3)).unwrap().symbol)
        .collect();
    assert_eq!(ruler, "08:00");
    // Monday's "Standup" tints its block cyan in column 0 (x = 1 + 6 = 7).
    assert_eq!(buf.get(Position::new(7, 4)).unwrap().bg, Color::Cyan);
    // The 13:30 "now" rule is drawn somewhere across the grid.
    assert!(
        buf.cells()
            .iter()
            .any(|c| c.symbol == '─' && c.fg == Color::Red)
    );

    print!("{}", terminal.backend());
}
