//! Exercises [`TraceWaterfall`]: a framed waterfall of a synthetic distributed
//! trace — a root HTTP span with nested DB, cache, and downstream spans at
//! increasing depth and start offsets, one row selected, including a
//! fractional sub-cell boundary glyph.
//!
//! The spans are plain caller-owned state — what an app's model would hold and
//! a reducer recomputes; [`TraceWaterfall`] only reads them (the pure
//! projection [`List`]/[`Tree`] use). Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example trace_waterfall_demo
//! ```

use rstui_core::{Color, Style, Terminal, TestBackend};
use rstui_widgets::{Block, TraceSpan, TraceWaterfall};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("TestBackend is infallible");

    // The flattened trace an app's model would own: a root HTTP request with
    // nested DB, cache, and a downstream call at increasing depth and start.
    let trace = || {
        [
            TraceSpan::new(0, 0, 120, "GET /checkout").style(Style::new().fg(Color::Cyan)),
            TraceSpan::new(1, 5, 40, "authz.check").style(Style::new().fg(Color::Green)),
            TraceSpan::new(1, 50, 55, "db.query orders").style(Style::new().fg(Color::Magenta)),
            TraceSpan::new(2, 58, 18, "cache.get").style(Style::new().fg(Color::Yellow)),
            TraceSpan::new(1, 95, 22, "POST payments.charge").style(Style::new().fg(Color::Red)),
        ]
    };

    terminal
        .draw(|frame| {
            frame.render_widget(
                TraceWaterfall::new(&trace()[..])
                    .total(Some(120))
                    .name_width(24)
                    .selected(Some(2))
                    .selected_style(Style::new().bg(Color::DarkGray))
                    .bar_style(Style::new().fg(Color::Blue))
                    .block(Block::bordered().title("trace 4f2a · 120ms")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
