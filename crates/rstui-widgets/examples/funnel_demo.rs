//! Exercises [`Funnel`] the way a growth dashboard does: a signup conversion
//! funnel with the per-stage conversion percentage shown, beside the same
//! stages with the percentage turned off — the centred bands narrowing toward
//! the bottom, label/value/percentage overlaid.
//!
//! The stages are plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`Funnel`] only reads them (the pure projection
//! [`List`]/[`BarChart`] use). Running over a [`TestBackend`] keeps it
//! TTY-free, so it doubles as a deterministic snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example funnel_demo
//! ```

use rstui_core::{Color, Constraint, Layout, Style, Terminal, TestBackend};
use rstui_widgets::{Block, Funnel, FunnelStage};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(60, 14)).expect("TestBackend is infallible");

    // The per-stage counts an app's model would own (a signup funnel).
    let signups = || {
        [
            FunnelStage::new(12_480, "Visitors"),
            FunnelStage::new(5_230, "Sign-ups"),
            FunnelStage::new(2_110, "Trials"),
            FunnelStage::new(640, "Paid"),
        ]
    };

    terminal
        .draw(|frame| {
            let [pct, raw] = Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                .areas(frame.area());

            // Conversion percentage of the first stage shown (the default).
            frame.render_widget(
                Funnel::new(signups())
                    .bar_style(Style::new().fg(Color::Cyan))
                    .block(Block::bordered().title("signups (conv %)")),
                pct,
            );

            // The same stages with just the raw counts.
            frame.render_widget(
                Funnel::new(signups())
                    .percent(false)
                    .bar_style(Style::new().fg(Color::Magenta))
                    .block(Block::bordered().title("signups (count)")),
                raw,
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
