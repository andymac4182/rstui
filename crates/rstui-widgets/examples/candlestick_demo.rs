//! Exercises [`Candlestick`] the way a trading dashboard does: an OHLC bar
//! series in a framed pane, auto-scaled and explicitly windowed.
//!
//! The candle series is plain caller-owned state — the ring buffer an app's
//! model would `push` a finished bar onto in `update`; [`Candlestick`] only
//! ever reads it (the same pure projection [`List`]/[`BarChart`] use). Running
//! over a [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example candlestick_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Candle, Candlestick};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(48, 14)).expect("TestBackend is infallible");

    // The finished bars an app's model would own (a market-data ring buffer).
    let bars = [
        Candle::new(10.0, 12.0, 9.0, 11.0),
        Candle::new(11.0, 11.5, 8.0, 8.5),
        Candle::new(8.5, 10.0, 8.0, 9.5),
        Candle::new(9.5, 14.0, 9.0, 13.0),
        Candle::new(13.0, 13.5, 11.0, 11.5),
        Candle::new(11.5, 12.0, 7.0, 7.5),
        Candle::new(7.5, 9.0, 7.0, 8.5),
        Candle::new(8.5, 15.0, 8.0, 14.5),
    ];

    terminal
        .draw(|frame| {
            let [auto, capped] = Layout::vertical([Constraint::Length(8), Constraint::Length(6)])
                .areas(frame.area());

            // Auto-scaled to the lowest low / highest high across the window.
            frame.render_widget(
                Candlestick::new(bars)
                    .candle_width(2)
                    .gap(1)
                    .block(Block::bordered().title("AAPL (auto)")),
                auto,
            );

            // The same bars against a fixed price window: anything outside
            // 8..14 clamps to the edge row (no panic — the totality rule).
            frame.render_widget(
                Candlestick::new(bars)
                    .bounds(Some([8.0, 14.0]))
                    .gap(1)
                    .bullish_style(Style::new().fg(Color::Cyan))
                    .bearish_style(Style::new().fg(Color::Magenta))
                    .block(Block::bordered().title("window 8..14")),
                capped,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
