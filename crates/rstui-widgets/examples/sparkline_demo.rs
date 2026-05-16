//! Exercises [`Sparkline`] the way a dashboard does: a label beside a compact
//! one-row trend of a caller-owned sample series, auto-scaled and explicitly
//! capped.
//!
//! The series is plain caller-owned state — the ring buffer an app's model
//! would `push` a sample onto in `update`; [`Sparkline`] only ever reads it
//! (the same pure projection [`List`]/[`Gauge`] use). Running over a
//! [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example sparkline_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Sparkline};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(40, 5)).expect("TestBackend is infallible");

    // The recent samples an app's model would own (a metrics ring buffer).
    let req_rate: [u64; 24] = [
        2, 4, 3, 6, 8, 7, 9, 12, 10, 14, 13, 18, 16, 20, 17, 11, 8, 6, 9, 13, 15, 19, 22, 21,
    ];

    terminal
        .draw(|frame| {
            let [auto, capped] = Layout::vertical([Constraint::Length(3), Constraint::Length(2)])
                .areas(frame.area());

            // Auto-scaled to the largest sample. Sparkline is a leaf
            // adornment with no Block of its own (like StatusBar), so the
            // caller frames it: render the Block, draw into its inner area.
            let frame_block = Block::bordered().title("req/s (auto)");
            let inner = frame_block.inner(auto);
            frame.render_widget(frame_block, auto);
            frame.render_widget(
                Sparkline::new(&req_rate).style(Style::new().fg(Color::Green)),
                inner,
            );

            // The same series against a fixed ceiling: everything over 16
            // clamps to a full block (no panic — the totality rule).
            let [label, trend] =
                Layout::horizontal([Constraint::Length(8), Constraint::Fill(1)]).areas(capped);
            frame.render_widget(Block::new().title("cap 16"), label);
            frame.render_widget(
                Sparkline::new(&req_rate)
                    .max(Some(16))
                    .style(Style::new().fg(Color::Cyan)),
                trend,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
