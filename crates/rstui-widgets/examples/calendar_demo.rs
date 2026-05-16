//! Exercises [`Calendar`] the way a scheduler would: a framed month grid whose
//! date facts are caller-owned and whose `selected`/`today` days the widget
//! only highlights.
//!
//! `Calendar` does **no date math** — the `year`, `month`, `day_count`, and
//! the weekday of day 1 are caller-owned inputs an app's model holds (computed
//! by the reducer or a date crate of the caller's choosing), exactly the pure
//! projection [`List`] uses for `selected`. No `chrono`/`time` dependency is
//! pulled in. Running over a [`TestBackend`] keeps it TTY-free, so it doubles
//! as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example calendar_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Calendar};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(25, 11)).expect("TestBackend is infallible");

    // The date facts an app's model owns: May 2026 has 31 days and the 1st
    // is a Friday (weekday index 5, with 0 = Sunday). The widget does no
    // arithmetic with these — it only lays them out.
    let (year, month, day_count, weekday_of_first) = (2026, 5, 31, 5);

    terminal
        .draw(|frame| {
            frame.render_widget(
                Calendar::new(year, month, day_count, weekday_of_first)
                    .today(Some(14))
                    .selected(Some(17))
                    .today_style(Style::new().fg(Color::Yellow))
                    .selected_style(Style::new().fg(Color::Black).bg(Color::Cyan))
                    .weekday_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("May 2026")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
