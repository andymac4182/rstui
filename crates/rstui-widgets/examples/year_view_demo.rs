//! Exercises [`YearView`] the way a planner's "year at a glance" pane does:
//! twelve mini-months tiled in a grid, each one a **reused**
//! [`Calendar`](rstui_widgets::Calendar), with a `today` accent, a selected
//! day, and caller-derived "busy" day dots.
//!
//! The per-month date facts (`(day_count, weekday_of_first)` pairs) and the
//! busy days are plain caller-owned state — the model an app would own and a
//! reducer would edit; [`YearView`] only ever reads it and does **no** date
//! math (exactly the pure projection [`Calendar`] uses). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example year_view_demo
//! ```

use rstui_core::{Color, Position, Style, Terminal, TestBackend};
use rstui_widgets::{Block, YearView};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(96, 32)).expect("TestBackend is infallible");

    // The per-month date facts an app's model owns for 2026: each pair is
    // `(day_count, weekday_of_first)` with 0 = Sunday. The widget does no
    // arithmetic with these — it hands each to a mini `Calendar`, which only
    // lays it out.
    let months = [
        (31, 4), // Jan 2026, 1st = Thu
        (28, 0), // Feb
        (31, 0), // Mar
        (30, 3), // Apr
        (31, 5), // May
        (30, 1), // Jun
        (31, 3), // Jul
        (31, 6), // Aug
        (30, 2), // Sep
        (31, 4), // Oct
        (30, 0), // Nov
        (31, 2), // Dec
    ];

    // Days the reducer derived from its event model as "busy" (month, dom).
    let busy = [(5_u32, 17_u32), (5, 18), (5, 22), (1, 1), (12, 25)];

    terminal
        .draw(|frame| {
            frame.render_widget(
                YearView::new(2026)
                    .months(&months)
                    .today(Some((5, 14)))
                    .selected(Some((5, 17)))
                    .busy(&busy)
                    .header_style(Style::new().fg(Color::Yellow))
                    .title_style(Style::new().fg(Color::Cyan))
                    .block(Block::bordered().title("2026")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    let backend = terminal.backend();
    let buf = backend.buffer();

    // Self-asserting smoke: the bordered frame, the centred "Year 2026"
    // title, a reused Calendar's header, the busy-day dot, and the hit-test.
    assert_eq!(buf.get(Position::new(0, 0)).unwrap().symbol, '\u{250c}');
    let inner_row1: String = (0..96)
        .map(|x| buf.get(Position::new(x, 1)).unwrap().symbol)
        .collect();
    assert!(
        inner_row1.contains("Year 2026"),
        "the title row reads 'Year 2026', got:\n{inner_row1}"
    );

    // month_at inverts the tiling; cell_rect is its companion. January is the
    // first cell, and a reused `Calendar` drew "January 2026" at its corner.
    let view = YearView::new(2026).months(&months).block(Block::bordered());
    let jan = view.cell_rect(buf.area(), 1);
    assert!(!jan.is_empty(), "January has a grid cell");
    assert_eq!(
        view.month_at(buf.area(), jan.position()),
        Some(1),
        "January's cell hit-tests back to month 1",
    );
    let jan_header: String = (jan.left()..jan.right())
        .map(|x| buf.get(Position::new(x, jan.top())).unwrap().symbol)
        .collect();
    assert!(
        jan_header.contains("January 2026"),
        "January's cell hosts a reused Calendar header, got: {jan_header:?}",
    );

    // A busy day in May (1st = Fri, weekday 5): the 17th carries a • dot.
    let mut found_dot = false;
    let may = view.cell_rect(buf.area(), 5);
    for y in may.top()..may.bottom() {
        for x in may.left()..may.right() {
            if buf.get(Position::new(x, y)).unwrap().symbol == '\u{2022}' {
                found_dot = true;
            }
        }
    }
    assert!(found_dot, "a busy day in May is dotted");

    print!("{backend}");
}
