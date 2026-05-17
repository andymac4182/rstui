//! Exercises [`Waterfall`] the way a finance dashboard does: a P&L bridge that
//! walks from revenue down through cost lines to operating income, beside a
//! horizontal sales-pipeline variance bridge — rises, falls, an absolute total
//! rule-off, and the thin connectors that join the walk.
//!
//! The steps are plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`Waterfall`] only reads them (the pure projection
//! [`List`]/[`BarChart`] use). Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example waterfall_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Waterfall, WaterfallDirection, WaterfallStep};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("TestBackend is infallible");

    // The P&L bridge an app's model would own (a quarterly walk, $k).
    let pnl = || {
        [
            WaterfallStep::delta(120, "Rev"),
            WaterfallStep::delta(-45, "COGS"),
            WaterfallStep::delta(-18, "S&M"),
            WaterfallStep::delta(-12, "R&D"),
            WaterfallStep::total("OpInc"),
        ]
    };

    // A sales-pipeline variance bridge (opening → closing), drawn sideways.
    let pipeline = || {
        [
            WaterfallStep::delta(80, "Open"),
            WaterfallStep::delta(35, "New"),
            WaterfallStep::delta(-14, "Slip"),
            WaterfallStep::delta(-9, "Lost"),
            WaterfallStep::total("Close"),
        ]
    };

    terminal
        .draw(|frame| {
            let [bridge, variance] =
                Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                    .areas(frame.area());

            // Vertical bridge: rises green, falls red, the total in cyan,
            // connectors dim.
            frame.render_widget(
                Waterfall::new(pnl())
                    .bar_gap(2)
                    .rise_style(Style::new().fg(Color::Green))
                    .fall_style(Style::new().fg(Color::Red))
                    .total_style(Style::new().fg(Color::Cyan))
                    .connector_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("P&L bridge ($k)")),
                bridge,
            );

            // The same idea sideways, with a fixed ceiling so the floating
            // ends land mid-cell and render a partial eighth-block glyph.
            frame.render_widget(
                Waterfall::new(pipeline())
                    .direction(WaterfallDirection::Horizontal)
                    .max(Some(140))
                    .bar_gap(1)
                    .rise_style(Style::new().fg(Color::Blue))
                    .fall_style(Style::new().fg(Color::Magenta))
                    .total_style(Style::new().fg(Color::Cyan))
                    .connector_style(Style::new().fg(Color::DarkGray))
                    .block(Block::bordered().title("pipeline /140")),
                variance,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
