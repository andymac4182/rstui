//! Exercises [`DateNavigator`] the way a calendar app's toolbar does: a
//! one-row strip pinned above the month grid, projecting the caller-owned
//! view-`mode` index and the caller-formatted period label.
//!
//! `DateNavigator` does **no date math** — `"May 2026"` is formatted by the
//! reducer (or a date crate of the caller's choosing, never `chrono`/`time`
//! here), and the highlighted `mode` is plain caller-owned state the widget
//! only reads. A click is mapped to a [`NavTarget`](rstui_widgets::NavTarget)
//! by `target_at` and dispatched to a reducer action — the same projection
//! [`Tabs`](rstui_widgets::Tabs)/[`List`](rstui_widgets::List) use. Running
//! over a [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example date_navigator_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Modifier, Position, Style, Terminal, TestBackend};
use rstui_widgets::{Block, DateNavigator, NavTarget, Paragraph};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(72, 6)).expect("TestBackend is infallible");

    // The toolbar state an app's model would own: the period label the
    // reducer formatted, and "Month" (index 2) as the active view.
    let label = "May 2026";
    let mode = 2usize;

    terminal
        .draw(|frame| {
            let rows =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(frame.area());

            frame.render_widget(
                DateNavigator::new(label)
                    .mode(mode)
                    .style(Style::new().fg(Color::White))
                    .label_style(Style::new().add_modifier(Modifier::BOLD))
                    .button_style(Style::new().fg(Color::DarkGray))
                    .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                rows[0],
            );

            // The body the toolbar sits above (a stand-in for the grid).
            frame.render_widget(
                Paragraph::new("(month grid for May 2026)")
                    .style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered()),
                rows[1],
            );
        })
        .expect("TestBackend is infallible");

    // --- Self-asserting checks (the deterministic smoke) ---

    let buf = terminal.backend().buffer().clone();
    let w = buf.area().width;
    let strip: String = (0..w)
        .map(|x| buf.get(Position::new(x, 0)).unwrap().symbol)
        .collect();

    // The prev `‹` control is anchored to the left edge of the strip row…
    assert_eq!(
        buf.get(Position::new(1, 0)).unwrap().symbol,
        '‹',
        "the prev control is at the left edge"
    );
    // …and the next `›` control is in the last three columns.
    assert_eq!(
        buf.get(Position::new(w - 2, 0)).unwrap().symbol,
        '›',
        "the next control is at the right edge"
    );

    // The strip carries the caller-formatted label, both buttons, and the
    // full mode switch (the demo width is sized so they all fit).
    assert!(strip.contains("May 2026"), "the caller's label is centred");
    assert!(strip.contains("Today"), "the Today button is drawn");
    assert!(strip.contains("＋ New"), "the New button is drawn");
    assert!(
        strip.contains("Month") && strip.contains("Agenda") && strip.contains('│'),
        "the segmented mode switch is drawn"
    );

    // `target_at` is the exact inverse of the render walk: each control
    // resolves to its `NavTarget`, so the app maps a click to a reducer
    // action without re-deriving the layout.
    let nav = DateNavigator::new(label).mode(mode);
    let area = buf.area();
    assert_eq!(
        nav.target_at(area, Position::new(1, 0)),
        Some(NavTarget::Prev)
    );
    assert_eq!(
        nav.target_at(area, Position::new(w - 2, 0)),
        Some(NavTarget::Next)
    );
    assert_eq!(
        nav.target_at(area, Position::new(4, 0)),
        Some(NavTarget::Today)
    );
    assert_eq!(
        nav.target_at(area, Position::new(13, 0)),
        Some(NavTarget::New)
    );
    // The centred label glyph is a drawn cell that is non-interactive.
    let m_x = (0..w)
        .find(|&x| buf.get(Position::new(x, 0)).unwrap().symbol == 'M')
        .expect("the label is drawn");
    assert_eq!(nav.target_at(area, Position::new(m_x, 0)), None);

    // The selected "Month" segment carries the accent background, and every
    // accented cell hit-tests back to `Mode(2)` (render ⇔ hit-test parity).
    let mut accented = 0usize;
    for x in 0..w {
        if buf.get(Position::new(x, 0)).unwrap().bg == Color::Cyan {
            accented += 1;
            assert_eq!(
                nav.target_at(area, Position::new(x, 0)),
                Some(NavTarget::Mode(2)),
                "an accented cell maps back to the selected mode"
            );
        }
    }
    assert!(accented >= 5, "the ` Month ` segment is fully accented");

    print!("{}", terminal.backend());
}
