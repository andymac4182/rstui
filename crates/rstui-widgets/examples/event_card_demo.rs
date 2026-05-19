//! Exercises [`EventCard`] the way a calendar app would: the detail body it
//! drops inside a [`Modal`] when you click an event.
//!
//! `EventCard` does **no date math** — the `day_label` is a caller-formatted
//! string an app's model holds (a multi-day event's whole `"19–21 May"` range
//! is the caller's to format), exactly the pure projection the calendar model
//! uses. No `chrono`/`time` dependency is pulled in. Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example event_card_demo
//! ```

use rstui_core::{Buffer, Color, Position, Rect, Style, Terminal, TestBackend, Widget};
use rstui_widgets::{Block, CalendarEvent, EventCard, Modal};

fn main() {
    // The caller-owned event the app's model holds. The card never computes
    // the date — "Mon 19 May" is the caller's string.
    let event = CalendarEvent::new(42, "Design review")
        .with_day(19)
        .with_span(14 * 60, 15 * 60 + 30) // 14:00–15:30
        .with_color(Color::Magenta)
        .with_location("Studio B / Zoom")
        .with_description(
            "Walk through the new onboarding flow end to end and agree the \
             cut-line for the next release.",
        );

    let mut terminal = Terminal::new(TestBackend::new(46, 12)).expect("TestBackend is infallible");

    terminal
        .draw(|frame| {
            let area = frame.area();
            // Pair the card with a Modal at the call site — the editor/card
            // never centre or clear themselves.
            let modal = Modal::new()
                .width(rstui_core::Constraint::Percentage(90))
                .height(rstui_core::Constraint::Percentage(90))
                .block(Block::bordered().title("Event"));
            let inner = modal.inner(area);
            frame.render_widget(modal, area);
            frame.render_widget(
                EventCard::new(&event)
                    .day_label("Mon 19 May")
                    .title_style(Style::new().fg(Color::Magenta))
                    .time_style(Style::new().fg(Color::Cyan))
                    .location_style(Style::new().fg(Color::DarkGray)),
                inner,
            );
        })
        .expect("TestBackend is infallible");

    // Self-assert: render once more into a bare buffer and check the card's
    // structure exactly (the example is its own deterministic smoke test).
    let mut buf = Buffer::empty(Rect::new(0, 0, 44, 8));
    EventCard::new(&event)
        .day_label("Mon 19 May")
        .render(buf.area(), &mut buf);

    // Row 0: the colour swatch (tinted the event colour) then the bold title.
    let dot = buf.get(Position::new(0, 0)).unwrap();
    assert_eq!(dot.symbol, '●');
    assert_eq!(dot.fg, Color::Magenta);
    assert_eq!(buf.get(Position::new(2, 0)).unwrap().symbol, 'D'); // "Design …"

    // Row 1: the caller's date label, the separator, the clock-formatted span.
    let row1: String = (0..30)
        .map(|x| buf.get(Position::new(x, 1)).unwrap().symbol)
        .collect();
    assert!(
        row1.starts_with("Mon 19 May · 14:00–15:30"),
        "row1 = {row1:?}"
    );

    // Row 2: the location line.
    let row2: String = (0..20)
        .map(|x| buf.get(Position::new(x, 2)).unwrap().symbol)
        .collect();
    assert!(row2.starts_with("📍 Studio B"), "row2 = {row2:?}");

    // Row 3: the divider rule.
    assert_eq!(buf.get(Position::new(0, 3)).unwrap().symbol, '─');

    // Row 4+: the wrapped description (reused Paragraph soft-wrap).
    assert_eq!(buf.get(Position::new(0, 4)).unwrap().symbol, 'W'); // "Walk …"

    print!("{}", terminal.backend());
    println!("event_card_demo: OK — swatch+title, date·time, location, divider, wrapped body");
}
